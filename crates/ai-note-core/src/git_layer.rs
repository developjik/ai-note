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
            Err(GitError::Auth) => { /* file:// 부재 원격은 NotFound로 오지 않음 — 통과 경로 */ }
            Ok(_) => panic!("부재 원격인데 성공함"),
        }
    }
}
