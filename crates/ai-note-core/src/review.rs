//! 리뷰 파이프라인 (M4 — 계획 §4 `review`, F21/F29/F32/§4.6/§4.9).
//!
//! 검토함(대기 변경 세트 목록·요약 카드)·승인(changeset::approve 위임 —
//! 원자 머지·'이미 반영됨' 판정 포함)·기각(작성자 표시명 매칭 되돌리기
//! 안내 F21)·이력(반영/기각/취소 + '조정 후 반영' 표시)·승인 후 해소 완성
//! (의미 변경 재확인 알림 R9)을 소유한다. 알림(반영·기각·검토 요청)은
//! 이 파이프라인이 생성한다(F6 귀속).

use crate::changeset::{self, ApproveOutcome, Changeset, CsState};
use crate::github::GithubService;
use crate::state::ChangesetStore;

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct ReviewCard {
    pub id: String,
    pub author_display: String,
    pub summary: String,
    pub origin_label: String,
    pub pr_number: Option<i64>,
    /// 조정(AI 해소)을 거친 변경 — 재확인 알림 대상(R9).
    pub needs_reconfirmation: bool,
    pub files: Vec<String>,
}

/// 기원 표시 라벨(A5/E4 — 일상 언어).
pub fn origin_label(origin: changeset::CsOrigin) -> String {
    match origin {
        changeset::CsOrigin::Edit => "직접 수정".into(),
        changeset::CsOrigin::Agent => "AI 작업".into(),
        changeset::CsOrigin::Resolution => "조정 후 재정리".into(),
    }
}

/// 검토함 — 대기 변경 세트를 카드로(작성자 표시명 포함 E5).
pub fn inbox(store: &ChangesetStore) -> Result<Vec<ReviewCard>, String> {
    let rows = store.list_by_state(CsState::PendingReview)?;
    Ok(rows
        .into_iter()
        .map(|(cs, pr)| ReviewCard {
            id: cs.id.clone(),
            author_display: cs.author_display.clone(),
            summary: cs.summary.clone(),
            origin_label: origin_label(cs.origin),
            pr_number: pr,
            needs_reconfirmation: cs.origin == changeset::CsOrigin::Resolution,
            files: cs.files.iter().map(|f| f.path.clone()).collect(),
        })
        .collect())
}

#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub enum ReviewAction {
    /// 반영 완료 — 이력+알림 '반영됨'.
    Applied { id: String, author_display: String },
    /// 다른 승인이 먼저 — 안내 '이미 반영됨'(F32).
    AlreadyApplied { id: String, author_display: String },
    /// 충돌 — 해소 시작(에이전트) + 재확인 알림 예약(R9).
    Resolving { id: String, author_display: String },
    /// 기각 — 작성자에게 되돌리기/수정 재제출 안내(F21).
    Rejected { id: String, author_display: String },
    /// 해소 상한 초과 — 자동 취소 + 알림(R7).
    Cancelled { id: String, author_display: String },
}

/// 승인 처리 — approve 위임 + 결과 분류(알림 생성은 여기서).
pub async fn approve(
    vault: &git2::Repository,
    gh: &GithubService,
    owner_repo: &str,
    remote_url: &str,
    pat: &str,
    store: &ChangesetStore,
    cs: &mut Changeset,
) -> Result<ReviewAction, String> {
    let author = cs.author_display.clone();
    let id = cs.id.clone();
    match changeset::approve(vault, gh, owner_repo, remote_url, pat, store, cs)
        .await
        .map_err(|e| e.to_string())?
    {
        ApproveOutcome::Applied => Ok(ReviewAction::Applied { id, author_display: author }),
        ApproveOutcome::AlreadyApplied => {
            Ok(ReviewAction::AlreadyApplied { id, author_display: author })
        }
        ApproveOutcome::NeedsResolution => {
            Ok(ReviewAction::Resolving { id, author_display: author })
        }
    }
}

/// 기각 — 상태 전이 + F21 안내 문구(작성자 표시명 대상).
pub fn reject(store: &ChangesetStore, cs: &mut Changeset) -> Result<ReviewAction, String> {
    let author = cs.author_display.clone();
    let id = cs.id.clone();
    changeset::reject(store, cs).map_err(|e| e.to_string())?;
    Ok(ReviewAction::Rejected { id, author_display: author })
}

/// 기각 안내 문구(F21 — 되돌리기/수정 후 재제출, 일상 언어·어휘 감사 대상).
pub fn reject_guidance(author_display: &str) -> String {
    format!("{author_display}님이 다시 손볼 수 있어요: 원문은 그대로 남아 있고, 고친 뒤 다시 검토로 보낼 수 있어요")
}

/// 해소 상한(R7) — 재시도 기록 갱신 후 상한 도달 시 취소.
/// 반환: Some(안내 문구)는 취소 발생(알림용).
pub fn record_resolution_failure(store: &ChangesetStore, cs: &mut Changeset) -> Result<Option<String>, String> {
    let retries = resolution_retries(store, &cs.id)? + 1;
    set_resolution_retries(store, &cs.id, retries)?;
    if retries >= changeset::RESOLUTION_MAX_RETRIES {
        changeset::transition(cs, CsState::Cancelled).map_err(|e| e.to_string())?;
        store.set_state(&cs.id, CsState::Cancelled).map_err(|e| e.to_string())?;
        return Ok(Some(format!(
            "{} 님의 변경을 자동으로 접었어요. 두 번 고친 부분이 계속 겹쳐서요 — 원문에서 직접 고친 뒤 다시 보내주시면 이어갈게요",
            cs.author_display
        )));
    }
    Ok(None)
}

fn resolution_retries(store: &ChangesetStore, id: &str) -> Result<u32, String> {
    store.get_resolution_retries(id)
}

fn set_resolution_retries(store: &ChangesetStore, id: &str, n: u32) -> Result<(), String> {
    store.set_resolution_retries(id, n)
}

/// 이력 조회 — 상태별(반영/기각/취소) + 표시명(F29/E5).
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct HistoryRow {
    pub id: String,
    pub author_display: String,
    pub summary: String,
    pub state_label: String,
    pub origin_label: String,
    pub pr_number: Option<i64>,
}

pub fn state_label(state: CsState) -> String {
    match state {
        CsState::Draft => "작성 중".into(),
        CsState::PendingReview => "검토 중".into(),
        CsState::Applied => "반영됨".into(),
        CsState::Rejected => "도로 돌림".into(),
        CsState::Resolving => "조정 중".into(),
        CsState::Cancelled => "자동 취소".into(),
    }
}

pub fn history(store: &ChangesetStore) -> Result<Vec<HistoryRow>, String> {
    let mut rows = Vec::new();
    for state in [CsState::Applied, CsState::Rejected, CsState::Cancelled] {
        for (cs, pr) in store.list_by_state(state)? {
            rows.push(HistoryRow {
                id: cs.id.clone(),
                author_display: cs.author_display.clone(),
                summary: cs.summary.clone(),
                state_label: state_label(state),
                origin_label: origin_label(cs.origin),
                pr_number: pr,
            });
        }
    }
    Ok(rows)
}

// ── 나란히 보기 — 단어 단위 차이(하이라이트 재료) ─────────────────────

/// 단어 단위 diff — LCS 기반. 반환 (구간들, 좌/우 강조 여부).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DiffChunk {
    pub left: String,
    pub right: String,
    /// 좌측(기존)에서 강조(=바뀜/삭제).
    pub changed_left: bool,
    /// 우측(새)에서 강조(=바뀜/추가).
    pub changed_right: bool,
}

/// 토큰화: 공백 유지(화면 정려) — 단어/문장부호 단위.
fn tokenize(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev_alnum = false;
    for (i, ch) in s.char_indices() {
        let alnum = ch.is_alphanumeric();
        if i > start && alnum != prev_alnum && (ch != ' ' || prev_alnum) {
            out.push(&s[start..i]);
            start = i;
        }
        prev_alnum = alnum;
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// 단어 단위 나란히 diff — LCS(공통 최장 부분수열) 정려 후 op 병합:
/// 연속된 삭제+추가를 한 '바뀜' 청크로 합쳐 좌우 하이라이트를 맞춘다.
pub fn word_diff(old: &str, new: &str) -> Vec<DiffChunk> {
    let a = tokenize(old);
    let b = tokenize(new);
    let (n, m) = (a.len(), b.len());
    if n * m > 4_000_000 {
        return vec![DiffChunk {
            left: old.to_string(),
            right: new.to_string(),
            changed_left: true,
            changed_right: true,
        }];
    }
    #[derive(Debug)]
    enum Op<'a> {
        Common(&'a str),
        Del(&'a str),
        Add(&'a str),
    }
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut ops: Vec<Op> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Common(a[i]));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            ops.push(Op::Del(a[i]));
            i += 1;
        } else {
            ops.push(Op::Add(b[j]));
            j += 1;
        }
    }
    while i < n {
        ops.push(Op::Del(a[i]));
        i += 1;
    }
    while j < m {
        ops.push(Op::Add(b[j]));
        j += 1;
    }
    // 병합: 연속 (Del* Add*|Add* Del*) 묶음 → 하나의 변경 청크,
    // 연속 Common 묶음 → 동일 청크(좌우 동일 문자열).
    let mut chunks: Vec<DiffChunk> = Vec::new();
    let mut common_buf = String::new();
    let mut del_buf = String::new();
    let mut add_buf = String::new();
    fn flush_common(buf: &mut String, chunks: &mut Vec<DiffChunk>) {
        if !buf.is_empty() {
            chunks.push(DiffChunk {
                left: buf.clone(),
                right: buf.clone(),
                changed_left: false,
                changed_right: false,
            });
            buf.clear();
        }
    }
    fn flush_change(del: &mut String, add: &mut String, chunks: &mut Vec<DiffChunk>) {
        if !del.is_empty() || !add.is_empty() {
            chunks.push(DiffChunk {
                left: del.clone(),
                right: add.clone(),
                changed_left: true,
                changed_right: true,
            });
            del.clear();
            add.clear();
        }
    }
    for op in ops {
        match op {
            Op::Common(t) => {
                flush_change(&mut del_buf, &mut add_buf, &mut chunks);
                common_buf.push_str(t);
            }
            Op::Del(t) => {
                flush_common(&mut common_buf, &mut chunks);
                del_buf.push_str(t);
            }
            Op::Add(t) => {
                flush_common(&mut common_buf, &mut chunks);
                add_buf.push_str(t);
            }
        }
    }
    flush_change(&mut del_buf, &mut add_buf, &mut chunks);
    flush_common(&mut common_buf, &mut chunks);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(cs_states: &[(String, CsState, i64, u32)]) -> ChangesetStore {
        let s = ChangesetStore::open_memory().unwrap();
        for (id, state, pr, retries) in cs_states {
            let mut cs = Changeset {
                id: id.clone(),
                author_display: "김하나".into(),
                summary: "회의록 정리".into(),
                base_commit: "b".into(),
                files: vec![changeset::CsFile {
                    path: "회의/a.md".into(),
                    content: Some("내용".into()),
                    binary_b64: None,
                }],
                origin: changeset::CsOrigin::Edit,
                state: CsState::PendingReview,
            };
            s.put(&cs, Some(*pr)).unwrap();
            if *retries > 0 {
                s.set_resolution_retries(id, *retries).unwrap();
            }
            if *state != CsState::PendingReview {
                s.set_state(id, *state).unwrap();
                cs.state = *state;
            }
        }
        s
    }

    #[test]
    fn m4_inbox_cards_with_author_and_origin() {
        let s = store_with(&[
            ("cs-1".into(), CsState::PendingReview, 11, 0),
            ("cs-2".into(), CsState::Applied, 12, 0),
        ]);
        let cards = inbox(&s).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].id, "cs-1");
        assert_eq!(cards[0].author_display, "김하나");
        assert_eq!(cards[0].pr_number, Some(11));
        assert_eq!(cards[0].origin_label, "직접 수정");
        assert!(!cards[0].needs_reconfirmation);
    }

    #[test]
    fn m4_history_rows_state_labels() {
        let s = store_with(&[
            ("a".into(), CsState::Applied, 1, 0),
            ("b".into(), CsState::Rejected, 2, 0),
            ("c".into(), CsState::Cancelled, 3, 0),
        ]);
        let rows = history(&s).unwrap();
        assert_eq!(rows.len(), 3);
        let labels: Vec<&str> = rows.iter().map(|r| r.state_label.as_str()).collect();
        assert!(labels.contains(&"반영됨"));
        assert!(labels.contains(&"도로 돌림"));
        assert!(labels.contains(&"자동 취소"));
    }

    #[test]
    fn m4_reject_guidance_everyday_language() {
        let g = reject_guidance("박둘");
        assert!(g.contains("박둘"));
        assert!(
            crate::ui_strings::audit_no_git_vocabulary(&g, crate::ui_strings::Scope::App)
                .is_empty()
        );
        for l in [
            state_label(CsState::Applied),
            state_label(CsState::Rejected),
            state_label(CsState::Cancelled),
            state_label(CsState::Resolving),
            origin_label(changeset::CsOrigin::Resolution),
        ] {
            assert!(
                crate::ui_strings::audit_no_git_vocabulary(&l, crate::ui_strings::Scope::App)
                    .is_empty(),
                "라벨 어휘 위반: {l}"
            );
        }
    }

    /// R7 — 해소 실패 기록 3회째에 취소+안내, 그 전엔 유지.
    #[test]
    fn m4_resolution_failure_cap_cancels_with_notice() {
        let s = store_with(&[("cs-r".into(), CsState::Resolving, 21, 2)]);
        let mut cs = Changeset {
            id: "cs-r".into(),
            author_display: "김하나".into(),
            summary: "s".into(),
            base_commit: "b".into(),
            files: vec![],
            origin: changeset::CsOrigin::Edit,
            state: CsState::Resolving,
        };
        // 3회째(기존 2 + 1) → 취소
        let notice = record_resolution_failure(&s, &mut cs).unwrap();
        assert!(notice.is_some());
        let text = notice.unwrap();
        assert!(text.contains("자동으로 접었어요"));
        assert!(text.contains("김하나"));
        assert_eq!(cs.state, CsState::Cancelled);

        // 2회째까지는 유지
        let s2 = store_with(&[("cs-q".into(), CsState::Resolving, 22, 1)]);
        let mut cs2 = Changeset {
            id: "cs-q".into(),
            author_display: "x".into(),
            summary: "s".into(),
            base_commit: "b".into(),
            files: vec![],
            origin: changeset::CsOrigin::Edit,
            state: CsState::Resolving,
        };
        let none = record_resolution_failure(&s2, &mut cs2).unwrap();
        assert!(none.is_none());
        assert_eq!(cs2.state, CsState::Resolving);
    }

    #[test]
    fn m4_word_diff_highlights_changed_words() {
        let chunks = word_diff("오늘 회의에서 예산을 승인했다", "오늘 회의에서 예산을 반려했다");
        assert_eq!(chunks.len() >= 2, true);
        let changed: Vec<&DiffChunk> = chunks.iter().filter(|c| c.changed_left).collect();
        assert!(!changed.is_empty());
        assert!(changed[0].left.contains("승인"));
        assert!(changed[0].right.contains("반려"));
        // 공통 부분은 강조 아님
        let common: Vec<&DiffChunk> = chunks.iter().filter(|c| !c.changed_left).collect();
        assert!(common.iter().any(|c| c.left.contains("오늘")));
        assert_eq!(common[0].left, common[0].right);
    }

    #[test]
    fn m4_word_diff_addition_and_deletion_only() {
        // 추가만
        let chunks = word_diff("회의록", "회의록 작성 완료");
        assert!(chunks.iter().any(|c| c.changed_right && c.right.contains("완료")));
        // 삭제만
        let chunks = word_diff("회의록 작성 완료", "회의록");
        assert!(chunks.iter().any(|c| c.changed_left && c.left.contains("완료")));
        // 동일
        let chunks = word_diff("같은 문장", "같은 문장");
        assert!(chunks.iter().all(|c| !c.changed_left && !c.changed_right));
    }

    #[test]
    fn m4_word_diff_korean_and_fallback() {
        // 한국어 교체+영어 혼합
        let chunks = word_diff("v1 릴리스 note", "v2 릴리스 note");
        let changed = chunks.iter().find(|c| c.changed_left).unwrap();
        assert!(changed.left.contains("v1") && changed.right.contains("v2"));
        // 과대 입력 폴백 — 통째 1청크
        let big_old = "단어 ".repeat(3000);
        let big_new = "다른 ".repeat(1500);
        let chunks = word_diff(&big_old, &big_new);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].changed_left && chunks[0].changed_right);
    }
}
