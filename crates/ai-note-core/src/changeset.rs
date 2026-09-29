//! 변경 세트 오케스트레이터 (M2 — D0 설계, F6/F20/F21/F32/R7).
//!
//! **유일 소유**: 상태 전이·cs/<id> 브랜치 수명주기·수렴 트리거·취소는
//! 이 모듈이 지시하고 git_layer(워커)·github(PR API)·agent(해소)·
//! review(판정)는 협력자다(D0 §6).
//!
//! 모든 수정(직접 편집·AI 산출·충돌 해소)은 변경 세트 하나로 표현되고
//! 변경 세트만이 반영 단위다(F6). 커밋은 cs/<id> 브랜치에만 — 로컬 main
//! 직접 커밋 금지. 상태는 SQLite에 영속(앱 재시작 복원 — 시나리오 7).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CsState {
    /// 작성 중(아직 제출 전) — 작성자만 보임.
    Draft,
    /// 검토 대기 — 팀 검토함에 표시.
    PendingReview,
    /// 반영 완료(원격 main에 머지됨).
    Applied,
    /// 기각 — 작성자 재제출 대기(F21, 원문 유지).
    Rejected,
    /// AI 충돌 해소 중(반영 시도 충돌 — D0 §4 정정: pending_review에서 발화).
    Resolving,
    /// 해소 상한 초과 자동 취소(R7) — 일상 언어 알림·재제출 유도.
    Cancelled,
}

/// 변경 세트 기원 — 요약 카드·이력 구분(E4 계획).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CsOrigin {
    /// 사람이 직접 편집.
    Edit,
    /// AI 에이전트 산출.
    Agent,
    /// AI 충돌 해소 산출.
    Resolution,
}

/// 변경 파일 한 건 — 무전환 커밋의 재료(D0 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CsFile {
    /// 볼트 내 상대 경로(예: "회의/2026-09-30.md").
    pub path: String,
    /// 새 내용(삭제는 None).
    pub content: Option<String>,
    /// 이미지 등 이진 자산(base64) — 자산 폴더 보관(F30).
    #[serde(default)]
    pub binary_b64: Option<String>,
}

/// 변경 세트 — 반영 단위(D0 §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Changeset {
    /// 식별자: cs/<id> 브랜치명에 그대로 쓰인다(예: 20260930-1430-김하나).
    pub id: String,
    /// 작성자 표시명(초대장 payload — E5/F32 기각 반환 대상).
    pub author_display: String,
    /// 일상 언어 요약(A5 휴리스틱 — 사람이 읽는 첫 줄).
    pub summary: String,
    /// 만든 시점의 원격 main 커밋 해시(base — 수렴 판정 기준).
    pub base_commit: String,
    /// 변경 파일 목록.
    pub files: Vec<CsFile>,
    pub origin: CsOrigin,
    pub state: CsState,
}

/// 상태 전이 표 — 허용 전이만. 그 외는 프로그래밍 오류로 거부(D0 §4).
pub fn can_transition(from: CsState, to: CsState) -> bool {
    use CsState::*;
    matches!(
        (from, to),
        (Draft, PendingReview)
            | (PendingReview, Applied)
            | (PendingReview, Rejected)
            | (PendingReview, Resolving) // 반영 시도 충돌 시(D0 정정)
            | (Rejected, PendingReview) // 작성자 재제출(F21)
            | (Resolving, PendingReview) // 해소 성공 → 갱신 cs 재검토
            | (Resolving, Cancelled) // 상한 초과(R7)
    )
}

pub fn transition(cs: &mut Changeset, to: CsState) -> Result<(), String> {
    if !can_transition(cs.state, to) {
        return Err(format!(
            "허용되지 않는 상태 변화예요: {:?} → {:?}",
            cs.state, to
        ));
    }
    cs.state = to;
    Ok(())
}

/// 해소 재시도 상한(R7).
pub const RESOLUTION_MAX_RETRIES: u32 = 3;

/// 파일 목록에서 요약 첫 줄 생성(A5 휴리스틱 — git 어휘 없는 일상 언어).
pub fn heuristic_summary(files: &[CsFile]) -> String {
    if files.is_empty() {
        return "빈 변경이에요".into();
    }
    let names: Vec<String> = files.iter().map(|f| file_display_name(&f.path)).collect();
    let created = files.iter().filter(|f| f.content.is_some()).count();
    let removed = files.len() - created;
    // 경로에서 폴더 맥락 붙이기 — 최상위 폴더 1개면 그 이름 사용
    let folder = files
        .first()
        .map(|f| f.path.split('/').next().unwrap_or(""))
        .filter(|s| !s.is_empty() && files.iter().all(|f| f.path.starts_with(&format!("{s}/"))))
        .map(|s| format!("({s}) "))
        .unwrap_or_default();
    match (created, removed) {
        (1, 0) => format!("{}문서 고침: {}", folder, names[0]),
        (c, 0) => format!("{}문서 {}개 고침", folder, c),
        (0, 1) => format!("{}문서 지움: {}", folder, names[0]),
        (0, r) => format!("{}문서 {}개 지움", folder, r),
        (c, r) => format!("{}문서 {}개 고치고 {}개 지움", folder, c, r),
    }
}

fn file_display_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).trim_end_matches(".md").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(state: CsState) -> Changeset {
        Changeset {
            id: "20260930-1430-김하나".into(),
            author_display: "김하나".into(),
            summary: "회의록 정리".into(),
            base_commit: "abc123".into(),
            files: vec![CsFile {
                path: "회의/2026-09-30.md".into(),
                content: Some("# 회의".into()),
                binary_b64: None,
            }],
            origin: CsOrigin::Edit,
            state,
        }
    }

    #[test]
    fn m2_state_machine_allows_exactly_d0_transitions() {
        use CsState::*;
        let allowed = [
            (Draft, PendingReview),
            (PendingReview, Applied),
            (PendingReview, Rejected),
            (PendingReview, Resolving),
            (Rejected, PendingReview),
            (Resolving, PendingReview),
            (Resolving, Cancelled),
        ];
        let all = [Draft, PendingReview, Applied, Rejected, Resolving, Cancelled];
        for from in all {
            for to in all {
                let expect = allowed.contains(&(from, to));
                assert_eq!(
                    can_transition(from, to),
                    expect,
                    "{from:?}→{to:?} 기대 {expect}"
                );
            }
        }
        // 종착 상태에서 나가는 간선 없음(applied·cancelled)
        assert!(!can_transition(Applied, PendingReview));
        assert!(!can_transition(Cancelled, PendingReview));
    }

    #[test]
    fn m2_full_lifecycle_direct_edit() {
        let mut cs = sample(CsState::Draft);
        transition(&mut cs, CsState::PendingReview).unwrap();
        transition(&mut cs, CsState::Applied).unwrap();
    }

    #[test]
    fn m2_reject_then_resubmit_keeps_content() {
        let mut cs = sample(CsState::PendingReview);
        let before = cs.files.clone();
        transition(&mut cs, CsState::Rejected).unwrap();
        transition(&mut cs, CsState::PendingReview).unwrap(); // F21 재제출
        assert_eq!(cs.files, before, "재제출은 원문 유지");
    }

    #[test]
    fn m2_conflict_resolution_path_and_r7_cap() {
        let mut cs = sample(CsState::PendingReview);
        transition(&mut cs, CsState::Resolving).unwrap(); // 반영 시도 충돌
        transition(&mut cs, CsState::PendingReview).unwrap(); // 해소 성공(1회)
        // 재충돌 → 해소 → ... 3회 후 취소(R7)
        for _ in 0..(RESOLUTION_MAX_RETRIES - 1) {
            transition(&mut cs, CsState::Resolving).unwrap();
            transition(&mut cs, CsState::PendingReview).unwrap();
        }
        transition(&mut cs, CsState::Resolving).unwrap();
        transition(&mut cs, CsState::Cancelled).unwrap();
    }

    #[test]
    fn m2_invalid_transition_rejected() {
        let mut cs = sample(CsState::Draft);
        assert!(transition(&mut cs, CsState::Applied).is_err());
        let mut done = sample(CsState::Applied);
        assert!(transition(&mut done, CsState::Rejected).is_err());
    }

    #[test]
    fn m2_heuristic_summary_everyday_language() {
        let single = vec![CsFile { path: "회의/2026-09-30.md".into(), content: Some("x".into()), binary_b64: None }];
        let s = heuristic_summary(&single);
        assert!(s.contains("회의록 정리") || s.contains("고침"), "요약: {s}");
        // 어휘 감사(A1) — 요약은 사용자 표시 문자열
        assert!(crate::ui_strings::audit_no_git_vocabulary(&s, crate::ui_strings::Scope::App).is_empty());

        let multi = vec![
            CsFile { path: "회의/a.md".into(), content: Some("x".into()), binary_b64: None },
            CsFile { path: "회의/b.md".into(), content: Some("y".into()), binary_b64: None },
        ];
        assert!(heuristic_summary(&multi).contains("2개"));

        let del = vec![CsFile { path: "메모/오래된.md".into(), content: None, binary_b64: None }];
        assert!(heuristic_summary(&del).contains("지움"));

        let mixed = vec![
            CsFile { path: "a.md".into(), content: Some("x".into()), binary_b64: None },
            CsFile { path: "b.md".into(), content: None, binary_b64: None },
        ];
        let s = heuristic_summary(&mixed);
        assert!(s.contains("고치고") && s.contains("지움"), "{s}");
    }
}

// ── M2: 오케스트레이션 (D0 §6 — 이 모듈이 유일 소유) ────────────────

use crate::git_layer::{self, GitError};
use crate::github::{GithubError, GithubService, MergeOutcome};
use crate::state::ChangesetStore;

#[derive(thiserror::Error, Debug)]
pub enum OrchestrateError {
    #[error("{0}")]
    Git(#[from] GitError),
    #[error("{0}")]
    Github(#[from] GithubError),
    #[error("{0}")]
    Store(String),
    #[error("{0}")]
    State(String),
}

/// 제출 결과 — 검토함 표시용.
#[derive(Debug, PartialEq)]
pub enum SubmitOutcome {
    /// 검토 요청 생성 완료(PR 번호).
    Submitted { pr_number: u64 },
}

/// 승인(반영) 결과 — 검토함·알림 표시용(F32 포함).
#[derive(Debug, PartialEq)]
pub enum ApproveOutcome {
    /// 이번 승인으로 반영됨.
    Applied,
    /// 다른 승인이 먼저 반영함 — '이미 반영됨' 안내(F32).
    AlreadyApplied,
    /// 반영 충돌 — AI 해소(resolving)로 전이. M3 에이전트가 이어받는다.
    NeedsResolution,
}

/// 변경 세트 제출 — 무전환 cs 커밋 → 원격 cs 브랜치 → PR 생성 → 영속.
/// 상태: Draft/Rejected → PendingReview(F21 재제출 동일 경로).
/// 단일 스레드 실행(테스트)용 조립형 — 앱 브리지는 아래 단계 분해판 사용
/// (Repository !Send — await 경계를 넘지 않게, S2).
pub async fn submit(
    vault: &git2::Repository,
    gh: &GithubService,
    owner_repo: &str,
    remote_url: &str,
    pat: &str,
    store: &ChangesetStore,
    cs: &mut Changeset,
) -> Result<SubmitOutcome, OrchestrateError> {
    submit_prepare(vault, remote_url, pat, cs)?;
    let pr_number = submit_pr(gh, owner_repo, cs).await?;
    submit_persist(store, cs, pr_number)?;
    Ok(SubmitOutcome::Submitted { pr_number })
}

/// 단계 1(동기·git) — 무전환 cs 커밋 + 원격 cs 브랜치 강제 갱신.
pub fn submit_prepare(
    vault: &git2::Repository,
    remote_url: &str,
    pat: &str,
    cs: &mut Changeset,
) -> Result<(), OrchestrateError> {
    if !matches!(cs.state, CsState::Draft | CsState::Rejected) {
        return Err(OrchestrateError::State(format!(
            "지금은 검토에 보낼 수 없는 상태예요: {:?}",
            cs.state
        )));
    }
    git_layer::commit_cs(vault, cs)?;
    let refspec = format!("+refs/heads/cs/{}:refs/heads/cs/{}", cs.id, cs.id);
    git_layer::push_branch(vault, remote_url, pat, &refspec)?;
    transition(cs, CsState::PendingReview).map_err(OrchestrateError::State)?;
    Ok(())
}

/// 단계 2(비동기·REST) — PR 생성. §4.7 메타데이터 최소(작성자·파일).
pub async fn submit_pr(
    gh: &GithubService,
    owner_repo: &str,
    cs: &Changeset,
) -> Result<u64, OrchestrateError> {
    let head = format!("cs/{}", cs.id);
    let body = format!(
        "작성자: {}\n변경: {}",
        cs.author_display,
        cs.files
            .iter()
            .map(|f| f.path.clone())
            .collect::<Vec<_>>()
            .join(", ")
    );
    gh.create_pr(owner_repo, &head, "main", &cs.summary, &body)
        .await
        .map_err(Into::into)
}

/// 단계 3(동기·영속) — SQLite 기록.
pub fn submit_persist(
    store: &ChangesetStore,
    cs: &Changeset,
    pr_number: u64,
) -> Result<(), OrchestrateError> {
    store
        .put(cs, Some(pr_number as i64))
        .map_err(OrchestrateError::Store)
}

/// 첫 승인 반영 시도 — D0 §3 순서:
/// (a) 원격 main 최신 확인 (b) 이미 반영됨 판정(F32)
/// (c) base 낡음 → 자동 수렴(merge_trees) (d) 충돌 → NeedsResolution
/// (e) GitHub 머지 API(원자성) → Applied / AlreadyApplied.
pub async fn approve(
    vault: &git2::Repository,
    gh: &GithubService,
    owner_repo: &str,
    remote_url: &str,
    pat: &str,
    store: &ChangesetStore,
    cs: &mut Changeset,
) -> Result<ApproveOutcome, OrchestrateError> {
    if cs.state != CsState::PendingReview {
        return Err(OrchestrateError::State(format!(
            "검토 대기 중인 변경만 반영할 수 있어요: {:?}",
            cs.state
        )));
    }
    let (_, pr_number) = store
        .get(&cs.id)
        .map_err(OrchestrateError::Store)?
        .ok_or_else(|| OrchestrateError::Store(format!("기록이 없는 변경 세트예요: {}", cs.id)))?;
    let pr_number = pr_number.ok_or_else(|| {
        OrchestrateError::Store(format!("검토 요청 번호가 기록되지 않았어요: {}", cs.id))
    })? as u64;

    // (a) 원격 main 최신
    let main_head = gh
        .default_branch_head(owner_repo)
        .await?
        .ok_or_else(|| OrchestrateError::Github(GithubError::Other("팀 노트 저장소가 아직 준비되지 않았어요".into())))?;
    let cs_ref = format!("refs/heads/cs/{}", cs.id);

    // (b) 이미 반영됨(F32)
    if git_layer::is_already_applied(vault, &cs_ref, &main_head)? {
        transition(cs, CsState::Applied).map_err(OrchestrateError::State)?;
        store.set_state(&cs.id, CsState::Applied).map_err(OrchestrateError::Store)?;
        return Ok(ApproveOutcome::AlreadyApplied);
    }

    // (c) base 낡음 → 자동 수렴
    if cs.base_commit != main_head {
        match git_layer::converge_cs(vault, &cs_ref, &main_head, &cs.author_display) {
            Ok(_) => {
                // 갱신된 cs 브랜치 재반영(수렴 커밋은 cs tip 위 부모 추가 — 빠른 앞참조)
                let refspec = format!("refs/heads/cs/{}:refs/heads/cs/{}", cs.id, cs.id);
                git_layer::push_branch(vault, remote_url, pat, &refspec)?;
                cs.base_commit = main_head.clone();
                store.put(cs, Some(pr_number as i64)).map_err(OrchestrateError::Store)?;
            }
            // (d) 충돌 → resolving(D0 §4 정정: pending_review에서 발화)
            Err(GitError::Conflict) => {
                transition(cs, CsState::Resolving).map_err(OrchestrateError::State)?;
                store.set_state(&cs.id, CsState::Resolving).map_err(OrchestrateError::Store)?;
                return Ok(ApproveOutcome::NeedsResolution);
            }
            Err(e) => return Err(e.into()),
        }
    }

    // (e) 머지 API(원자성 — 이중 반영은 GitHub가 차단)
    let outcome = gh.merge_pr(owner_repo, pr_number).await?;
    match outcome {
        MergeOutcome::Merged => {
            transition(cs, CsState::Applied).map_err(OrchestrateError::State)?;
            store.set_state(&cs.id, CsState::Applied).map_err(OrchestrateError::Store)?;
            Ok(ApproveOutcome::Applied)
        }
        MergeOutcome::AlreadyMerged => {
            transition(cs, CsState::Applied).map_err(OrchestrateError::State)?;
            store.set_state(&cs.id, CsState::Applied).map_err(OrchestrateError::Store)?;
            Ok(ApproveOutcome::AlreadyApplied)
        }
    }
}

/// 기각(F21) — 원문 유지, 작성자 재제출 대기.
pub fn reject(store: &ChangesetStore, cs: &mut Changeset) -> Result<(), OrchestrateError> {
    transition(cs, CsState::Rejected).map_err(OrchestrateError::State)?;
    store.set_state(&cs.id, CsState::Rejected).map_err(OrchestrateError::Store)?;
    Ok(())
}
