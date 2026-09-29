//! AI Note Tauri 셸 — 프런트엔드에 노출되는 명령 브리지.
//!
//! 문자열·에러는 코어 ui_strings을 경유해 일상 언어로만 내보낸다(A1).
//! M1: 온보딩(claude 감지·수동 안내·초대장 연결) 명령 추가.

use serde::Serialize;
use std::sync::Mutex;
use tauri::Manager;

/// 모든 git 조작 직렬화(D 단일 작성자 — S2 !Send 원칙의 런타임 보강).
/// 코어가 함수별 &Repository를 받는 한 동시 호출이 같은 저장소를 물 수
/// 있어 브리지 경계에서 잠근다.
static GIT_OPS_LOCK: Mutex<()> = Mutex::new(());

#[tauri::command]
fn ui_string(key: String) -> String {
    ai_note_core::ui_strings::s(&key, ai_note_core::ui_strings::Scope::App).to_string()
}

#[tauri::command]
fn app_version() -> String {
    ai_note_core::CORE_VERSION.to_string()
}

#[derive(Serialize)]
pub struct ClaudeDetect {
    pub state: String, // ready | outdated | notInstalled
    pub version: String,
}

#[tauri::command]
fn detect_claude() -> ClaudeDetect {
    
    match ai_note_core::agent_install::detect() {
        ai_note_core::agent_install::InstallState::Ready { version } => {
            ClaudeDetect { state: "ready".into(), version }
        }
        ai_note_core::agent_install::InstallState::Outdated { version } => {
            ClaudeDetect { state: "outdated".into(), version }
        }
        ai_note_core::agent_install::InstallState::NotInstalled => {
            ClaudeDetect { state: "notInstalled".into(), version: String::new() }
        }
    }
}

fn app_data(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("노트 보관함 위치를 정하지 못했어요: {e}"))
}

// ── M2: 워크스페이스(읽기·검색·저장→변경 세트) ────────────────────

fn open_vault(app: &tauri::AppHandle) -> Result<git2::Repository, String> {
    let dir = app_data(app)?;
    let state = ai_note_core::state::load(&dir);
    let repo = state
        .connected_repo
        .ok_or_else(|| "팀 노트에 먼저 연결해 주세요".to_string())?;
    git2::Repository::open(dir.join(&repo)).map_err(|e| format!("노트 보관함을 열지 못했어요: {e}"))
}

fn github_from_custody(app: &tauri::AppHandle) -> Result<(ai_note_core::github::GithubService, String, String), String> {
    // (서비스, owner/repo, pat) — pat는 즉시 소비후기 내 콜백에서만 사용
    let dir = app_data(app)?;
    let state = ai_note_core::state::load(&dir);
    let repo = state
        .connected_repo
        .ok_or_else(|| "팀 노트에 먼저 연결해 주세요".to_string())?;
    let owner = state
        .owner
        .ok_or_else(|| "계정 정보가 없어요. 다시 연결해 주세요".to_string())?;
    let pat = ai_note_core::token_custody::load_pat().map_err(|e| e.to_string())?;
    let gh = ai_note_core::github::GithubService::new(&pat)
        .map_err(|e| e.to_string())?;
    Ok((gh, format!("{owner}/{repo}"), pat))
}

#[tauri::command]
fn workspace_list(app: tauri::AppHandle, dir: String) -> Result<Vec<ai_note_core::vault::TreeEntry>, String> {
    let vault = open_vault(&app)?;
    ai_note_core::vault::list_dir(&vault, &dir)
}
// 읽기 경로는 git2 객체 조회만 사용(인덱스 조작 아님) — 락 없이 안전.

#[tauri::command]
fn workspace_read(app: tauri::AppHandle, path: String) -> Result<String, String> {
    let vault = open_vault(&app)?;
    let bytes = ai_note_core::vault::read_file(&vault, &path)?;
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

#[tauri::command]
fn workspace_search(app: tauri::AppHandle, query: String) -> Result<Vec<ai_note_core::search::SearchHit>, String> {
    let vault = open_vault(&app)?;
    let index = ai_note_core::search::build_index(&vault)?;
    index.search(&query, 20)
}

#[derive(serde::Deserialize, Clone)]
pub struct ImagePayload {
    pub name: String,
    pub b64: String,
}

#[derive(serde::Serialize)]
pub struct SaveResult {
    pub pr_number: i64,
    pub summary: String,
}

#[tauri::command]
async fn workspace_save(
    app: tauri::AppHandle,
    path: String,
    content: String,
    image: Option<ImagePayload>,
) -> Result<SaveResult, String> {
    // (gh, owner_repo, pat) — 자격·원격 주소(동기)
    let (gh, owner_repo, pat) = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || github_from_custody(&app))
            .await
            .map_err(|e| e.to_string())?
    }?;
    let owner_repo = owner_repo.clone(); // 단계 1 클로저로 이동됨

    // 단계 1(동기·git): cs 구성 + 무전환 커밋 + 원격 반영 — Repository는
    // 이 블록 안에서만 삶(S2 !Send, await 경계 통과 금지).
    let owner_repo_inner = owner_repo.clone();
    let (cs, _store_dir) = {
        let app2 = app.clone();
        let path = path.clone();
        let content = content.clone();
        let image = image.clone();
        let owner_repo = owner_repo_inner;
        tauri::async_runtime::spawn_blocking(move || -> Result<(ai_note_core::changeset::Changeset, std::path::PathBuf), String> {
            let _guard = GIT_OPS_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            let vault = open_vault(&app2)?;
            let dir = app_data(&app2)?;
            let author = ai_note_core::token_custody::load_display().map_err(|e| e.to_string())?;
            let remote_url = remote_url_of(&vault, &owner_repo);

            let base = vault
                .find_reference("refs/heads/main")
                .and_then(|r| r.peel_to_commit())
                .map_err(|e| format!("노트 기준점을 찾지 못했어요: {e}"))?
                .id()
                .to_string();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let mut files = vec![ai_note_core::changeset::CsFile {
                path,
                content: Some(content),
                binary_b64: None,
            }];
            // 이미지 선택(선택) — 자산 폴더로(F30)
            if let Some(img) = &image {
                files.push(ai_note_core::changeset::CsFile {
                    path: format!("자산/{}", img.name),
                    content: None,
                    binary_b64: Some(img.b64.clone()),
                });
            }
            let summary = ai_note_core::changeset::heuristic_summary(&files);
            let mut cs = ai_note_core::changeset::Changeset {
                id: format!("{now}-{author}"),
                author_display: author,
                summary,
                base_commit: base,
                files,
                origin: ai_note_core::changeset::CsOrigin::Edit,
                state: ai_note_core::changeset::CsState::Draft,
            };
            ai_note_core::changeset::submit_prepare(&vault, &remote_url, &pat, &mut cs)
                .map_err(|e| e.to_string())?;
            Ok((cs, dir))
            // owner_repo는 이 클로저로 이동됨 — 이후 단계는 인자로 전달
        })
        .await
        .map_err(|e| e.to_string())?
    }?;

    // 단계 2(비동기·REST): PR 생성 — Repository 없음
    let pr_number = ai_note_core::changeset::submit_pr(&gh, &owner_repo, &cs)
        .await
        .map_err(|e| e.to_string())?;

    // 단계 3(동기·영속)
    let summary = cs.summary.clone();
    let cs_id = cs.id.clone();
    {
        let app3 = app.clone();
        tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
            let dir = app_data(&app3)?;
            let store = ai_note_core::state::ChangesetStore::open(&dir.join("state.db"))
                .map_err(|e| e.to_string())?;
            ai_note_core::changeset::submit_persist(&store, &cs, pr_number)
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
    }
    Ok(SaveResult {
        pr_number: pr_number as i64,
        summary: format!("{summary} (cs/{cs_id})"),
    })
}

fn remote_url_of(vault: &git2::Repository, owner_repo: &str) -> String {
    vault
        .find_remote("origin")
        .ok()
        .and_then(|r| r.url().map(|u| u.to_string()))
        .unwrap_or_else(|| format!("https://github.com/{owner_repo}.git"))
}

#[tauri::command]
fn account_display() -> Result<String, String> {
    ai_note_core::token_custody::load_display()
        .map_err(|_| "연결된 계정이 없어요".to_string())
}

#[tauri::command]
fn connected_repo(app: tauri::AppHandle) -> String {
    ai_note_core::state::load(&app_data(&app).unwrap_or_default())
        .connected_repo
        .unwrap_or_default()
}

#[tauri::command]
fn disconnect_account(app: tauri::AppHandle) -> Result<(), String> {
    ai_note_core::token_custody::clear_all().map_err(|e| e.to_string())?;
    if let Ok(dir) = app_data(&app) {
        let _ = ai_note_core::state::save(
            &dir,
            &ai_note_core::state::AppState::default(),
        );
    }
    Ok(())
}

#[tauri::command]
fn subscription_state() -> String {
    match ai_note_core::subscription::detect() {
        ai_note_core::subscription::SubscriptionState::LoggedIn => "loggedIn".into(),
        ai_note_core::subscription::SubscriptionState::NeedsLogin => "needsLogin".into(),
    }
}

#[tauri::command]
fn subscription_guide() -> Vec<(String, String)> {
    ai_note_core::subscription::login_guide_steps()
}

#[tauri::command]
fn claude_manual_steps() -> Vec<(String, String)> {
    let os = if cfg!(target_os = "macos") { "macos" } else { "windows" };
    ai_note_core::agent_install::manual_guide_steps(os)
}

#[derive(Serialize)]
pub struct ConnectResult {
    pub result: String, // admin | member
    pub repo: String,
}

#[tauri::command]
async fn connect_invitation(
    app: tauri::AppHandle,
    invitation: String,
    passphrase: String,
) -> Result<ConnectResult, String> {
    let vault_root = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("노트 보관함 위치를 정하지 못했어요: {e}"))?;
    let env = ai_note_core::onboarding::ConnectEnv::default();
    ai_note_core::onboarding::connect_and_record(&env, &invitation, &passphrase, &vault_root)
        .await
        .map(|out| match out {
            ai_note_core::onboarding::ConnectOutcome::AdminInitialized { repo, .. } => {
                ConnectResult { result: "admin".into(), repo }
            }
            ai_note_core::onboarding::ConnectOutcome::MemberConnected { repo, .. } => {
                ConnectResult { result: "member".into(), repo }
            }
        })
        .map_err(|e| e.to_string()) // ConnectError 표시 문자열은 이미 일상 언어
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            ui_string,
            app_version,
            detect_claude,
            claude_manual_steps,
            subscription_state,
            subscription_guide,
            workspace_list,
            workspace_read,
            workspace_search,
            workspace_save,
            account_display,
            connected_repo,
            disconnect_account,
            connect_invitation
        ])
        .run(tauri::generate_context!())
        .expect("앱을 시작하지 못했어요");
}
