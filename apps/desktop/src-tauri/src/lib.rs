//! AI Note Tauri 셸 — 프런트엔드에 노출되는 명령 브리지.
//!
//! 문자열·에러는 코어 ui_strings을 경유해 일상 언어로만 내보낸다(A1).
//! M1: 온보딩(claude 감지·수동 안내·초대장 연결) 명령 추가.

use serde::Serialize;
use tauri::Manager;

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
            account_display,
            connected_repo,
            disconnect_account,
            connect_invitation
        ])
        .run(tauri::generate_context!())
        .expect("앱을 시작하지 못했어요");
}
