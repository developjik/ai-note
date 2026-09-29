//! git 연산 계층 — git2(libgit2) in-process (M1 구현 — 계획 §4 `git`).
//!
//! S2 증명 기반: `Repository`는 `!Send` → 모든 조작은 단일 소유 워커로.
//! 시스템 git 실행파일 의존 금지(P3). ghp는 **콜백 클로저 안에서만** 존재
//! (환경변수·디스크 비기록, F14).
//!
//! M1 범위: 초대장 연결 후 원격 저장소 클론 + 볼트 초기화 + 최초 푸시.
//! 커밋·PR 파이프라인은 M2(changeset)가 소유한다.

use anyhow::Context;
use git2::{Cred, RemoteCallbacks, Repository};
use std::path::Path;

#[derive(thiserror::Error, Debug)]
pub enum GitError {
    #[error("노트 저장소에 연결하지 못했어요. 인터넷 상태를 확인해 주세요")]
    Network(String),
    #[error("초대장의 노트 보관함 권한이 부족해요")]
    Auth,
    #[error("{0}")]
    Other(String),
    /// 3-way 수렴 충돌 — resolving 상태로(resolving 전이 트리거).
    #[error("두 변경이 같은 부분을 고쳐서 자동으로 합치지 못했어요")]
    Conflict,
}

/// PAT 인증 콜백 — https 원격에 ghp를 user 비밀번호로 주입한다.
///
/// 토큰 문자열은 이 콜백 안에서만 살고, 실패 2회 후 시도를 끊는다
/// (GitHub는 잘못된 PAT에 반복 인증 시도를 401로 응답한다).
fn pat_credentials(pat: &str) -> impl FnMut(&str, Option<&str>, git2::CredentialType) -> std::result::Result<Cred, git2::Error> + '_ {
    let mut attempts = 0;
    move |_url, _user, _kind| {
        attempts += 1;
        if attempts > 2 {
            return Err(git2::Error::from_str("auth failed"));
        }
        Cred::userpass_plaintext("oauth2", pat)
    }
}

/// 원격 저장소를 목적지로 클론한다(볼트 다운로드 — E2E-3).
pub fn clone_vault(url: &str, pat: &str, dest: &Path) -> std::result::Result<Repository, GitError> {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(pat_credentials(pat));
    let mut fo = git2::FetchOptions::new();
    fo.remote_callbacks(callbacks);
    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fo);
    builder
        .clone(url, dest)
        .map_err(|e| classify_git_err(e, "클론"))
}

/// 기존 볼트를 다시 연다(초대장 재사용·재접속 F26).
pub fn open_vault(path: &Path) -> std::result::Result<Repository, GitError> {
    Repository::open(path).map_err(|e| classify_git_err(e, "열기"))
}

/// 새 팀 볼트 초기화 — 최초 커밋 + main 푸시 + 원격 HEAD를 main으로.
///
/// S2 교훈 반영: bare 원격 HEAD가 기본 분기를 가리키지 않으면 이후 클론이
/// 실패한다. 앱이 저장소를 생성한 직후(A3) 이 초기화를 수행한다.
pub fn init_and_push_first(vault: &Repository, url: &str, pat: &str, team_display: &str) -> std::result::Result<(), GitError> {
    let work = vault.workdir().context("bare 저장소는 초기화 대상 아님").map_err(|e| GitError::Other(e.to_string()))?;

    // 최소 볼트 구조: 환영 노트 + 자산 폴더(F30)
    std::fs::create_dir_all(work.join("자산"))
        .map_err(|e| GitError::Other(format!("자산 폴더 생성 실패: {e}")))?;
    // git은 빈 폴더를 보관하지 않는다 — 자산 폴더 유지용 키퍼(F30)
    std::fs::write(work.join("자산/.gitkeep"), "")
        .map_err(|e| GitError::Other(format!("자산 폴더 준비 실패: {e}")))?;
    std::fs::write(
        work.join("시작하기.md"),
        format!(
            "# {team_display} 팀 노트\n\n이곳에 팀의 모든 노트가 쌓입니다.\n화면 왼쪽에서 새 문서를 만들어 보세요.\n"
        ),
    )
    .map_err(|e| GitError::Other(format!("환영 노트 작성 실패: {e}")))?;

    {
        let mut index = vault
            .index()
            .map_err(|e| GitError::Other(format!("인덱스 열기 실패: {e}")))?;
        index
            .add_path(Path::new("시작하기.md"))
            .map_err(|e| GitError::Other(format!("시작 노트 추가 실패: {e}")))?;
        index
            .add_path(Path::new("자산/.gitkeep"))
            .map_err(|e| GitError::Other(format!("자산 폴더 보관 실패: {e}")))?;
        let tree_id = index
            .write_tree()
            .map_err(|e| GitError::Other(format!("트리 작성 실패: {e}")))?;
        let tree = vault
            .find_tree(tree_id)
            .map_err(|e| GitError::Other(e.to_string()))?;
        let sig = vault
            .signature()
            .or_else(|_| git2::Signature::now("AI Note", "app@local"))
            .map_err(|e| GitError::Other(e.to_string()))?;
        let first = vault
            .commit(Some("HEAD"), &sig, &sig, "볼트 시작", &tree, &[])
            .map_err(|e| GitError::Other(format!("최초 저장 실패: {e}")))?;
        let commit = vault
            .find_commit(first)
            .map_err(|e| GitError::Other(e.to_string()))?;
        vault
            .branch("main", &commit, true)
            .map_err(|e| GitError::Other(format!("기본 준비 실패: {e}")))?;
    }
    vault
        .set_head("refs/heads/main")
        .map_err(|e| GitError::Other(e.to_string()))?;

    // 첫 푸시 + 원격 HEAD 정리
    push_branch(vault, url, pat, "refs/heads/main:refs/heads/main")?;
    set_remote_head_main(url, pat);
    Ok(())
}

/// 지정 브랜치를 원격으로 올린다(커밋·PR 파이프라인은 M2).
pub fn push_branch(vault: &Repository, url: &str, pat: &str, refspec: &str) -> std::result::Result<(), GitError> {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(pat_credentials(pat));
    let mut remote = vault
        .remote_anonymous(url)
        .map_err(|e| GitError::Other(e.to_string()))?;
    remote
        .push(&[refspec], None)
        .map_err(|e| classify_git_err(e, "올리기"))
}

/// 원격(bare)의 HEAD를 main으로 지정 — 이후 기기의 클론이 main을 기본으로
/// 잡도록 한다(S2 발견 반영). libgit2에 원격 HEAD 직접 조작 API가 없어
/// 로컬 bare에서는 symbolic ref 갱신으로 수행한다.
/// (https 원격의 HEAD는 GitHub API 기본 분기 설정으로 M1 브리지가 처리)
fn set_remote_head_main(url: &str, _pat: &str) {
    if let Some(path) = url.strip_prefix("file://") {
        if let Ok(remote_repo) = Repository::open_bare(path) {
            let _ = remote_repo.reference_symbolic("HEAD", "refs/heads/main", true, "main 기본 분기 설정");
        }
    }
}

fn classify_git_err(e: git2::Error, what: &str) -> GitError {
    // git2 0.20: 인증 실패는 code=Auth(class는 None/Http 등)로 온다
    if e.code() == git2::ErrorCode::Auth {
        return GitError::Auth;
    }
    match e.class() {
        git2::ErrorClass::Net | git2::ErrorClass::Http => {
            GitError::Network(format!("{what} 네트워크 오류: {e}"))
        }
        _ => GitError::Other(format!("{what} 실패: {e}")),
    }
}

// ── M2: 변경 세트 cs/<id> 파이프라인 (D0 §2 — 무전환) ──────────────

/// cs 브랜치 커밋 생성 — 체크아웃 전환 없이: 메모리 인덱스에 base 트리 적재
/// → 파일 반영 → 트리 확정 → cs/<id> 브랜치 커밋(base 단일 부모).
/// 반환 = cs 커밋 해시. S2 교훈(전환=충돌 원천) 반영.
pub fn commit_cs(
    vault: &Repository,
    cs: &crate::changeset::Changeset,
) -> std::result::Result<String, GitError> {
    let base_oid = git2::Oid::from_str(&cs.base_commit)
        .map_err(|e| GitError::Other(format!("기준점 해석 실패: {e}")))?;
    let base_commit = vault
        .find_commit(base_oid)
        .map_err(|e| GitError::Other(format!("기준점 조회 실패: {e}")))?;
    let base_tree = base_commit.tree().map_err(|e| GitError::Other(e.to_string()))?;

    // libgit2: add_frombuffer는 저장소 소속 인덱스만 허용 → vault.index()
    // 사용(디스크 write는 호출하지 않음 — 메모리 상에서만 조작 후 트리 확정).
    let mut index = vault.index().map_err(|e| GitError::Other(e.to_string()))?;
    index
        .read_tree(&base_tree)
        .map_err(|e| GitError::Other(format!("기준 트리 적재 실패: {e}")))?;
    for f in &cs.files {
        match &f.content {
            Some(content) => {
                index
                    .add_frombuffer(
                        &git2::IndexEntry {
                            ctime: git2::IndexTime::new(0, 0),
                            mtime: git2::IndexTime::new(0, 0),
                            dev: 0,
                            ino: 0,
                            mode: 0o100644,
                            uid: 0,
                            gid: 0,
                            file_size: content.len() as u32,
                            id: git2::Oid::zero(),
                            flags: 0,
                            flags_extended: 0,
                            path: f.path.clone().into_bytes(),
                        },
                        content.as_bytes(),
                    )
                    .map_err(|e| GitError::Other(format!("문서 반영 실패({}): {e}", f.path)))?;
            }
            None => {
                index
                    .remove_path(std::path::Path::new(&f.path))
                    .map_err(|e| GitError::Other(format!("문서 제거 실패({}): {e}", f.path)))?;
            }
        }
    }
    let tree_id = index
        .write_tree_to(vault)
        .map_err(|e| GitError::Other(format!("트리 확정 실패: {e}")))?;
    // 인덱스 원상복구(HEAD 트리 재적재 — 디스크 무기록)
    if let Ok(head) = vault.head() {
        if let Ok(c) = head.peel_to_commit() {
            if let Ok(t) = c.tree() {
                let _ = index.read_tree(&t);
            }
        }
    }
    let tree = vault
        .find_tree(tree_id)
        .map_err(|e| GitError::Other(e.to_string()))?;

    let sig = git2::Signature::now(&cs.author_display, "app@local")
        .map_err(|e| GitError::Other(e.to_string()))?;
    let branch_ref = format!("refs/heads/cs/{}", cs.id);
    // D0 '단일 커밋 스택': cs 브랜치는 항상 base 위 단일 커밋. 재제출은
    // 커밋 객체를 새로 만들어 참조를 강제 이동(git2 commit()의
    // tip-부모 규칙 회피 — 스택이 아니라 교체다).
    let oid = vault
        .commit(None, &sig, &sig, &cs.summary, &tree, &[&base_commit])
        .map_err(|e| GitError::Other(format!("cs 저장 실패: {e}")))?;
    let oid_obj = git2::Oid::from_str(&oid.to_string()).unwrap();
    match vault.find_reference(&branch_ref) {
        Ok(mut r) => {
            r.set_target(oid_obj, "재제출 갱신")
                .map_err(|e| GitError::Other(format!("cs 갱신 실패: {e}")))?;
        }
        Err(_) => {
            vault
                .reference(&branch_ref, oid_obj, false, "cs 생성")
                .map_err(|e| GitError::Other(format!("cs 생성 실패: {e}")))?;
        }
    }
    Ok(oid.to_string())
}

/// cs 트리 조회 — 수렴·이미반영됨 판정 재료.
fn cs_tree<'a>(vault: &'a Repository, cs_ref: &str) -> std::result::Result<git2::Tree<'a>, GitError> {
    let ref_obj = vault
        .find_reference(cs_ref)
        .map_err(|e| GitError::Other(format!("cs 조회 실패: {e}")))?;
    let commit = ref_obj
        .peel_to_commit()
        .map_err(|e| GitError::Other(e.to_string()))?;
    commit.tree().map_err(|e| GitError::Other(e.to_string()))
}

/// '이미 반영됨' 판정(F32) — cs가 바꾼 모든 파일이 main에서 이미 동일한
/// 내용(동일 blob 해시)이면 true. GitHub 머지 원자성이 이중 반영을 막지만
/// 늦은 승인의 안내 표시('이미 반영됨')를 여기서 판정한다.
pub fn is_already_applied(
    vault: &Repository,
    cs_ref: &str,
    main_oid: &str,
) -> std::result::Result<bool, GitError> {
    let tree = cs_tree(vault, cs_ref)?;
    let main_commit_oid = git2::Oid::from_str(main_oid)
        .map_err(|e| GitError::Other(format!("main 해석 실패: {e}")))?;
    let main_commit = vault
        .find_commit(main_commit_oid)
        .map_err(|e| GitError::Other(e.to_string()))?;
    let main_tree = main_commit.tree().map_err(|e| GitError::Other(e.to_string()))?;

    let cs_ref_obj = vault
        .find_reference(cs_ref)
        .map_err(|e| GitError::Other(e.to_string()))?;
    let base = cs_ref_obj
        .peel_to_commit()
        .map_err(|e| GitError::Other(e.to_string()))?
        .parent(0)
        .map_err(|e| GitError::Other(e.to_string()))?;
    let base_tree = base.tree().map_err(|e| GitError::Other(e.to_string()))?;

    // cs가 base 대비 바꾼 파일들만 검사
    let diff = vault
        .diff_tree_to_tree(Some(&base_tree), Some(&tree), None)
        .map_err(|e| GitError::Other(e.to_string()))?;
    let deltas = diff.deltas();
    if deltas.len() == 0 {
        return Ok(false); // 빈 cs — 판정 대상 아님
    }
    for delta in deltas {
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let cs_blob = tree.get_path(std::path::Path::new(&path)).map(|e| e.id());
        let main_blob = main_tree.get_path(std::path::Path::new(&path)).map(|e| e.id());
        match (cs_blob, main_blob) {
            (Ok(a), Ok(b)) if a == b => { /* 동일 — 이미 반영됨 */ }
            (Err(_), Err(_)) => { /* 둘 다 없음(삭제 반영됨) */ }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// 자동 수렴(D0 §3) — 낡은 base의 cs를 최신 main에 3-way 병합(트리 수준
/// `merge_trees`, 무전환). 충돌 없으면 cs 브랜치를 부모 2개(cs tip + main)
/// 커밋으로 갱신하고 새 해시 반환. 충돌이면 Conflict → resolving로.
pub fn converge_cs(
    vault: &Repository,
    cs_ref: &str,
    main_oid: &str,
    author_display: &str,
) -> std::result::Result<String, GitError> {
    let cs_commit = vault
        .find_reference(cs_ref)
        .map_err(|e| GitError::Other(format!("cs 조회 실패: {e}")))?
        .peel_to_commit()
        .map_err(|e| GitError::Other(e.to_string()))?;
    let main_commit_oid = git2::Oid::from_str(main_oid)
        .map_err(|e| GitError::Other(format!("main 해석 실패: {e}")))?;
    let main_commit = vault
        .find_commit(main_commit_oid)
        .map_err(|e| GitError::Other(e.to_string()))?;
    let ancestor_oid = cs_commit.parent(0).map_err(|e| GitError::Other(e.to_string()))?.id();
    let ancestor = vault
        .find_commit(ancestor_oid)
        .map_err(|e| GitError::Other(e.to_string()))?;

    let ours = main_commit.tree().map_err(|e| GitError::Other(e.to_string()))?;
    let theirs = cs_commit.tree().map_err(|e| GitError::Other(e.to_string()))?;
    let base = ancestor.tree().map_err(|e| GitError::Other(e.to_string()))?;

    let mut merged_idx = vault
        .merge_trees(&base, &ours, &theirs, None)
        .map_err(|e| GitError::Other(format!("수렴 병합 실패: {e}")))?;
    if merged_idx.has_conflicts() {
        return Err(GitError::Conflict);
    }
    let tree_id = merged_idx
        .write_tree_to(vault)
        .map_err(|e| GitError::Other(format!("수렴 트리 확정 실패: {e}")))?;
    let tree = vault
        .find_tree(tree_id)
        .map_err(|e| GitError::Other(e.to_string()))?;
    let sig = git2::Signature::now(author_display, "app@local")
        .map_err(|e| GitError::Other(e.to_string()))?;
    let oid = vault
        .commit(
            Some(cs_ref),
            &sig,
            &sig,
            "최신 노트에 맞춰 정리됨",
            &tree,
            &[&cs_commit, &main_commit],
        )
        .map_err(|e| GitError::Other(format!("수렴 저장 실패: {e}")))?;
    Ok(oid.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 로컬 bare 원격 + 앱 측 초기화 → 다른 기기(클론)가 main을 기본으로 잡는지.
    #[test]
    fn m1_vault_init_then_clone_defaults_to_main() {
        let tmp = tempfile::tempdir().unwrap();
        let origin_path = tmp.path().join("origin.git");
        std::fs::create_dir_all(&origin_path).unwrap();
        Repository::init_bare(&origin_path).unwrap();
        let url = format!("file://{}", origin_path.canonicalize().unwrap().display());

        // 앱 측: 임시 클론에서 init_and_push_first 수행
        let staging = tmp.path().join("app-side");
        let vault = Repository::init(&staging).unwrap();
        init_and_push_first(&vault, &url, "ghp_dummy", "우리팀").unwrap();

        // 다른 기기: 클론 → 기본 분기 main + 시작 노트 존재
        let other = tmp.path().join("member-device");
        let cloned = clone_vault(&url, "ghp_dummy", &other).unwrap();
        let head = cloned.head().unwrap();
        assert_eq!(head.shorthand().unwrap(), "main");
        assert!(other.join("시작하기.md").exists());
        assert!(other.join("자산").is_dir());
    }

    /// PAT 인증 콜백: 자격 생성 성공 + 재시도 상한(잘못된 토큰 무한 루프 방지).
    /// (oauth2 사용자명 주입은 실 https 원격 E2E — wiremock 불가 영역 — M1 브리지
    /// 실계정 시나리오에서 검증한다.)
    #[test]
    fn m1_pat_callback_retries_are_bounded() {
        let mut cb = pat_credentials("ghp_bad");
        assert!(cb("https://github.com/x/y.git", None, git2::CredentialType::USER_PASS_PLAINTEXT).is_ok());
        assert!(cb("u", None, git2::CredentialType::USER_PASS_PLAINTEXT).is_ok());
        assert!(cb("u", None, git2::CredentialType::USER_PASS_PLAINTEXT).is_err());
    }

    /// 없는 원격 주소 → 친화 네트워크 오류 분류.
    #[test]
    fn m1_clone_bad_remote_is_friendly_error() {
        let tmp = tempfile::tempdir().unwrap();
        let url = format!("file://{}/nope.git", tmp.path().display());
        let err = clone_vault(&url, "ghp_x", &tmp.path().join("dst"));
        // file:// 부재 원격은 NotFound 계열 — Other/Network 어느 쪽이든 친구 메시지여야
        match err {
            Err(GitError::Network(m)) | Err(GitError::Other(m)) => {
                assert!(m.contains("클론") || m.contains("연결"), "메시지: {m}");
            }
            Err(GitError::Auth) | Err(GitError::Conflict) => { /* 통과 경로 */ }
            Ok(_) => panic!("부재 원격인데 성공함"),
        }
    }

    // ── M2: cs 파이프라인 통합 테스트(로컬 bare 원격 형상) ────────────

    fn setup_origin_and_vault() -> (tempfile::TempDir, Repository, String) {
        let tmp = tempfile::tempdir().unwrap();
        let origin_path = tmp.path().join("origin.git");
        std::fs::create_dir_all(&origin_path).unwrap();
        Repository::init_bare(&origin_path).unwrap();
        let url = format!("file://{}", origin_path.canonicalize().unwrap().display());
        let staging = tmp.path().join("vault");
        let vault = Repository::init(&staging).unwrap();
        init_and_push_first(&vault, &url, "ghp_t", "팀").unwrap();
        // 원격 최신 main을 로컬에 갱신(fetch 없이 재클론이 곧 원격 상태)
        let vault = Repository::open(&staging).unwrap();
        (tmp, vault, url)
    }

    fn cs_fixture(id: &str, base: &str, path: &str, content: &str) -> crate::changeset::Changeset {
        crate::changeset::Changeset {
            id: id.into(),
            author_display: "김하나".into(),
            summary: "테스트 변경".into(),
            base_commit: base.into(),
            files: vec![crate::changeset::CsFile { path: path.into(), content: Some(content.into()) }],
            origin: crate::changeset::CsOrigin::Edit,
            state: crate::changeset::CsState::PendingReview,
        }
    }

    fn head_of(repo: &Repository, r: &str) -> String {
        repo.find_reference(r).unwrap().peel_to_commit().unwrap().id().to_string()
    }

    /// 무전환 cs 커밋이 base 트리에 파일만 반영해 생성되는가(작업 트리 무변경).
    #[test]
    fn m2_commit_cs_no_checkout() {
        let (_tmp, vault, _url) = setup_origin_and_vault();
        let main = head_of(&vault, "refs/heads/main");
        let cs = cs_fixture("2026-1", &main, "새문서.md", "# 새 문서");
        let oid = commit_cs(&vault, &cs).unwrap();
        // cs 브랜치 존재 + 부모 = base(main)
        let commit = vault.find_commit(git2::Oid::from_str(&oid).unwrap()).unwrap();
        assert_eq!(commit.parent_count(), 1);
        assert_eq!(commit.parent(0).unwrap().id().to_string(), main);
        // 작업 디렉터리는 그대로(무전환)
        assert!(!vault.workdir().unwrap().join("새문서.md").exists());
        // 트리에 반영됨
        let tree = commit.tree().unwrap();
        assert!(tree.get_path(std::path::Path::new("새문서.md")).is_ok());
    }

    /// 시나리오 2(부분): 이미 main에 동일 내용이 있으면 '이미 반영됨'.
    #[test]
    fn m2_already_applied_detection() {
        let (_tmp, vault, _url) = setup_origin_and_vault();
        let main0 = head_of(&vault, "refs/heads/main");

        // cs 생성(문서 신규)
        let cs = cs_fixture("2026-2", &main0, "가이드.md", "# 가이드");
        let cs_oid = commit_cs(&vault, &cs).unwrap();
        // 아직 main에 없음 → false
        assert!(!is_already_applied(&vault, "refs/heads/cs/2026-2", &main0).unwrap());

        // main에 동일 내용 반영(수동으로 main 브랜치에 cs 내용 커밋)
        apply_to_main(&vault, "가이드.md", "# 가이드", &cs_oid);
        let main1 = head_of(&vault, "refs/heads/main");
        assert!(is_already_applied(&vault, "refs/heads/cs/2026-2", &main1).unwrap());
    }

    fn apply_to_main(vault: &Repository, path: &str, content: &str, cs_parent: &str) {
        let main_commit = vault.find_reference("refs/heads/main").unwrap().peel_to_commit().unwrap();
        let cs_commit = vault
            .find_commit(git2::Oid::from_str(cs_parent).unwrap())
            .unwrap();
        let mut index = vault.index().unwrap();
        index.read_tree(&main_commit.tree().unwrap()).unwrap();
        index
            .add_frombuffer(
                &git2::IndexEntry {
                    ctime: git2::IndexTime::new(0, 0),
                    mtime: git2::IndexTime::new(0, 0),
                    dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
                    file_size: content.len() as u32,
                    id: git2::Oid::zero(), flags: 0, flags_extended: 0,
                    path: path.as_bytes().to_vec(),
                },
                content.as_bytes(),
            )
            .unwrap();
        let tree_id = index.write_tree_to(vault).unwrap();
        let tree = vault.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("main", "app@local").unwrap();
        vault
            .commit(Some("refs/heads/main"), &sig, &sig, "반영", &tree, &[&main_commit, &cs_commit])
            .unwrap();
    }

    /// 시나리오 3: cs base가 낡았지만 충돌 없는 파일이면 자동 수렴(부모 2개).
    #[test]
    fn m2_converge_stale_base_clean() {
        let (_tmp, vault, _url) = setup_origin_and_vault();
        let main0 = head_of(&vault, "refs/heads/main");

        // cs-A: 문서A 추가(base=main0)
        let cs_a = cs_fixture("2026-a", &main0, "문서A.md", "# A");
        commit_cs(&vault, &cs_a).unwrap();
        // cs-B: 문서B 추가(base=main0) → 먼저 main에 반영됨
        let cs_b = cs_fixture("2026-b", &main0, "문서B.md", "# B");
        let b_oid = commit_cs(&vault, &cs_b).unwrap();
        apply_to_main(&vault, "문서B.md", "# B", &b_oid);
        let main1 = head_of(&vault, "refs/heads/main");

        // cs-A 수렴: base(main0) ≠ main1이지만 파일이 다름 → 자동 병합 성공
        let new_oid = converge_cs(&vault, "refs/heads/cs/2026-a", &main1, "김하나").unwrap();
        let conv = vault.find_commit(git2::Oid::from_str(&new_oid).unwrap()).unwrap();
        assert_eq!(conv.parent_count(), 2, "수렴 커밋은 부모 2개");
        // 수렴 트리에 A와 B 둘 다
        let t = conv.tree().unwrap();
        assert!(t.get_path(std::path::Path::new("문서A.md")).is_ok());
        assert!(t.get_path(std::path::Path::new("문서B.md")).is_ok());
    }

    /// 시나리오 4 진입: 같은 파일 같은 줄 충돌 → Conflict → resolving 유도.
    #[test]
    fn m2_converge_conflict_detected() {
        let (_tmp, vault, _url) = setup_origin_and_vault();
        let main0 = head_of(&vault, "refs/heads/main");

        // cs-X: 시작하기.md 내용 교체
        let cs_x = cs_fixture("2026-x", &main0, "시작하기.md", "# 우리 팀 규칙\n1. 정시");
        let x_oid = commit_cs(&vault, &cs_x).unwrap();
        // main에서 같은 파일을 다르게 교체(다른 cs가 먼저 반영됨)
        apply_to_main(&vault, "시작하기.md", "# 우리 팀 규칙\n1. 자율", &x_oid);
        let main1 = head_of(&vault, "refs/heads/main");

        let err = converge_cs(&vault, "refs/heads/cs/2026-x", &main1, "김하나");
        match err {
            Err(GitError::Conflict) => { /* resolving 경로 유도 — 성공 */ }
            other => panic!("충돌이어야 함: {other:?}"),
        }
    }
}
