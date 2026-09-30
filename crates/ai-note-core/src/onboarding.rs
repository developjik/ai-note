//! 온보딩 연결 오케스트레이터 (M1 — E2E-3, F13/F16/F26/A3/A7).
//!
//! 초대장 문자열 + 암호 → 연결 완료까지의 단일 흐름:
//! 디코드 → ghp·표시명 보관 → PAT 검증 → 저장소 확보(자동 생성/재사용)
//! → 볼트 다운로드(클론) 또는 초기화+최초 푸시.
//! 첫 사용자(관리자)는 초기화 경로, 이후 팀원은 클론 경로(F16 혼합 모델 —
//! 첫 사용자 여부는 이미 원격 저장소가 존재하는지로 결정된다).

use crate::github::{GithubService, RepoOutcome};
use crate::git_layer;
use crate::invite;
use crate::token_custody;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
pub enum ConnectOutcome {
    /// 관리자 경로 — 저장소를 새로 만들고 볼트를 초기화했다(F16 최초 1인).
    AdminInitialized { repo: String, vault_path: PathBuf },
    /// 팀원 경로 — 기존 저장소를 내려받았다.
    MemberConnected { repo: String, vault_path: PathBuf },
}

#[derive(thiserror::Error, Debug)]
pub enum ConnectError {
    #[error("초대장을 읽지 못했어요: {0}")]
    Invite(#[from] invite::InviteError),
    #[error("{0}")]
    Github(#[from] crate::github::GithubError),
    #[error("{0}")]
    Git(#[from] crate::git_layer::GitError),
    #[error("계정 정보를 안전하게 보관하지 못했어요: {0}")]
    Custody(#[from] crate::token_custody::CustodyError),
    #[error("노트 보관함 위치를 정할 수 없어요: {0}")]
    VaultPath(String),
}

/// 연결 환경 — 프로덕션(github.com)이 기본. 통합 테스트가 wiremock API
/// 주소와 로컬 bare 원격을 주입해 E2E-3 형상을 재현한다.
#[derive(Default, Clone)]
pub struct ConnectEnv {
    /// GitHub API 기준 주소(None = github.com).
    pub api_base: Option<String>,
    /// 원격 저장소 URL 기준(None = https://github.com).
    pub remote_base: Option<String>,
}

/// 초대장 연결 — vault_root 아래 <repo> 디렉터리로 볼트를 구성한다.
///
/// 이미 로컬 볼트가 있으면 다시 열기(초대장 재사용 F26 — 같은 폴더 재사용).
pub async fn connect(
    invitation: &str,
    passphrase: &str,
    vault_root: &Path,
) -> Result<ConnectOutcome, ConnectError> {
    connect_in(&ConnectEnv::default(), invitation, passphrase, vault_root).await
}

/// connect_in + 로컬 상태 기록(설정 화면용). vault_root의 상위가 앱 데이터
/// 디렉터리로 간주해 app-state.json에 연결 결과를 남긴다.
pub async fn connect_and_record(
    env: &ConnectEnv,
    invitation: &str,
    passphrase: &str,
    vault_root: &Path,
) -> Result<ConnectOutcome, ConnectError> {
    connect_in_recorded(env, invitation, passphrase, vault_root).await
}

/// connect_in + 연결 결과 로컬 기록(설정 화면·저장 파이프라인용).
pub async fn connect_in_recorded(
    env: &ConnectEnv,
    invitation: &str,
    passphrase: &str,
    vault_root: &Path,
) -> Result<ConnectOutcome, ConnectError> {
    let (out, owner) = connect_in_owner(env, invitation, passphrase, vault_root).await?;
    let state = crate::state::AppState {
        connected_repo: Some(match &out {
            ConnectOutcome::AdminInitialized { repo, .. } => repo.clone(),
            ConnectOutcome::MemberConnected { repo, .. } => repo.clone(),
        }),
        owner: Some(owner),
        is_admin: matches!(out, ConnectOutcome::AdminInitialized { .. }),
    };
    // 상태 기록 실패는 연결 실패가 아니다 — 결과는 유지
    let _ = crate::state::save(vault_root, &state);
    Ok(out)
}

pub async fn connect_in(
    env: &ConnectEnv,
    invitation: &str,
    passphrase: &str,
    vault_root: &Path,
) -> Result<ConnectOutcome, ConnectError> {
    connect_in_owner(env, invitation, passphrase, vault_root).await.map(|(out, _)| out)
}

async fn connect_in_owner(
    env: &ConnectEnv,
    invitation: &str,
    passphrase: &str,
    vault_root: &Path,
) -> Result<(ConnectOutcome, String), ConnectError> {
    // 1) 디코드 (S4 코덱)
    let payload = invite::decode(invitation, passphrase)?;

    // 2) 안전 보관 (F14 — 이후 모든 계층은 보관소에서만 꺼내 쓴다)
    token_custody::store_pat(&payload.ghp)?;
    token_custody::store_display(&payload.display)?;

    // 3) PAT 검증
    let gh = GithubService::new_at(&payload.ghp, env.api_base.as_deref())?;
    let user = gh.verify_token().await?;

    // 4) 저장소 확보 — 자동 생성(A3) 또는 재사용(422)
    let outcome = gh.ensure_repo(&payload.repo).await?;

    let remote_base = env.remote_base.as_deref().unwrap_or("https://github.com");
    let url = format!("{}/{}/{}.git", remote_base.trim_end_matches('/'), user.login, payload.repo);
    let vault_path = vault_root.join(&payload.repo);

    // 5) 볼트 확보 — 기존 원격 내용이 있으면 클론, 빈 저장소면 관리자 초기화
    match outcome {
        RepoOutcome::Existing => {
            // 이미 로컬에 있으면 재접속(F26), 없으면 클론(E2E-3)
            if vault_path.join(".git").exists() {
                git_layer::open_vault(&vault_path)?;
            } else {
                git_layer::clone_vault(&url, &payload.ghp, &vault_path)?;
            }
            Ok((
                ConnectOutcome::MemberConnected {
                    repo: payload.repo,
                    vault_path,
                },
                user.login,
            ))
        }
        RepoOutcome::Created => {
            // 방금 만든 빈 저장소 → 관리자 초기화 + 최초 푸시
            std::fs::create_dir_all(&vault_path)
                .map_err(|e| ConnectError::VaultPath(e.to_string()))?;
            let repo = git2_init(&vault_path)?;
            git_layer::init_and_push_first(&repo, &url, &payload.ghp, &payload.display)?;
            Ok((
                ConnectOutcome::AdminInitialized {
                    repo: payload.repo,
                    vault_path,
                },
                user.login,
            ))
        }
    }
}

fn git2_init(path: &Path) -> Result<git2::Repository, ConnectError> {
    git2::Repository::init(path).map_err(|e| ConnectError::VaultPath(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// 실제 키체인을 공유하는 테스트 직렬화(병렬 clear_all 충돌 방지).
    static CUSTODY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 코덱 수준 통합: 잘못된 암호 → 초대장 단계에서 안전 거부(원격 도달 전).
    #[tokio::test]
    async fn m1_connect_rejects_bad_passphrase_before_network() {
        let _guard = CUSTODY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let payload = invite::InvitePayload {
            ghp: "ghp_will_not_be_used".into(),
            repo: "never".into(),
            display: "김하나".into(),
        };
        let inv = invite::encode(&payload, "정답-암호").unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let err = connect_in(&ConnectEnv::default(), &inv, "오답-암호", tmp.path())
            .await
            .unwrap_err();
        assert!(matches!(err, ConnectError::Invite(_)));
        // 네트워크/GitHub 오류가 아니라 초대장 단계에서 끝났다
    }

    /// E2E-3 관리자 경로: wiremock GitHub(201 Created) + 로컬 bare 원격 →
    /// 저장소 생성 판정 → 볼트 초기화 + 최초 푸시 + 원격 HEAD main.
    #[tokio::test]
    async fn m1_e2e3_admin_initializes_vault() {
        let _guard = CUSTODY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _ = token_custody::clear_all();
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "login": "team-ainote", "id": 1
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/user/repos"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "full_name": "team-ainote/team-notes", "private": true
            })))
            .mount(&server)
            .await;

        // 로컬 bare 원격(= 원격 저장소 역할)
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("remote/team-ainote");
        std::fs::create_dir_all(origin.join("team-notes.git")).unwrap();
        git2::Repository::init_bare(origin.join("team-notes.git")).unwrap();

        let payload = invite::InvitePayload {
            ghp: "ghp_adminpath".into(),
            repo: "team-notes".into(),
            display: "첫 관리자".into(),
        };
        let inv = invite::encode(&payload, "팀-암호").unwrap();
        let env = ConnectEnv {
            api_base: Some(server.uri()),
            remote_base: Some(git_layer::local_remote(&tmp.path().join("remote"))),
        };
        let vault_root = tmp.path().join("vaults");

        let out = connect_in(&env, &inv, "팀-암호", &vault_root).await.unwrap();
        match &out {
            ConnectOutcome::AdminInitialized { repo, vault_path } => {
                assert_eq!(repo, "team-notes");
                assert!(vault_path.join("시작하기.md").exists());
            }
            other => panic!("관리자 경로여야 함: {other:?}"),
        }
        // 원격에 main + 환영 노트 도달
        let origin_repo = git2::Repository::open_bare(origin.join("team-notes.git")).unwrap();
        assert_eq!(origin_repo.head().unwrap().target().is_some(), true);
        // 보관소에 계정 반영(F14)
        assert_eq!(token_custody::load_display().unwrap(), "첫 관리자");
        let _ = token_custody::clear_all();
    }

    /// E2E-3 팀원 경로: 원격에 내용이 있고 저장소 422(재사용) → 클론 수행.
    #[tokio::test]
    async fn m1_e2e3_member_clones_existing_vault() {
        let _guard = CUSTODY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _ = token_custody::clear_all();
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "login": "team-ainote", "id": 1
            })))
            .mount(&server)
            .await;
        // 422 = 이미 존재(F26 재사용)
        Mock::given(method("POST")).and(path("/user/repos"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "message": "name already exists on this account"
            })))
            .mount(&server)
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let remote_root = tmp.path().join("remote");
        let origin_path = remote_root.join("team-ainote/team-notes.git");
        std::fs::create_dir_all(&origin_path).unwrap();
        let origin = git2::Repository::init_bare(&origin_path).unwrap();

        // 관리자가 이미 만든 내용: 시작 노트 커밋을 origin main에 심는다
        let staging = tmp.path().join("seed");
        let seed = git2::Repository::init(&staging).unwrap();
        std::fs::write(staging.join("관리자-노트.md"), "# 이미 있음").unwrap();
        {
            let mut index = seed.index().unwrap();
            index.add_path(std::path::Path::new("관리자-노트.md")).unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = seed.find_tree(tree_id).unwrap();
            let sig = git2::Signature::now("seed", "seed@local").unwrap();
            let c = seed
                .commit(Some("HEAD"), &sig, &sig, "시드", &tree, &[])
                .unwrap();
            let commit = seed.find_commit(c).unwrap();
            seed.branch("main", &commit, true).unwrap();
        }
        seed.set_head("refs/heads/main").unwrap();
        {
            let mut remote = seed.remote("origin", &git_layer::local_remote(&origin_path)).unwrap();
            remote.push(&["refs/heads/main:refs/heads/main"], None).unwrap();
        }
        origin.set_head("refs/heads/main").unwrap();

        let payload = invite::InvitePayload {
            ghp: "ghp_memberpath".into(),
            repo: "team-notes".into(),
            display: "팀원 둘".into(),
        };
        let inv = invite::encode(&payload, "팀-암호").unwrap();
        let env = ConnectEnv {
            api_base: Some(server.uri()),
            remote_base: Some(git_layer::local_remote(&remote_root)),
        };
        let vault_root = tmp.path().join("vaults");

        let out = connect_in(&env, &inv, "팀-암호", &vault_root).await.unwrap();
        match &out {
            ConnectOutcome::MemberConnected { repo, vault_path } => {
                assert_eq!(repo, "team-notes");
                assert!(vault_path.join("관리자-노트.md").exists(), "클론된 내용이 보여야 함");
            }
            other => panic!("팀원 경로여야 함: {other:?}"),
        }
        let _ = token_custody::clear_all();
    }
}
