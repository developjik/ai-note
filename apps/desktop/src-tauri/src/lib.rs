//! AI Note Tauri 셸 — 프런트엔드에 노출되는 명령 브리지 (M0 골격).
//!
//! 문자열·에러는 코어 ui_strings을 경유해 일상 언어로만 내보낸다(A1).
//! M1+에서 온보딩/워크스페이스/리뷰 명령이 이 목록에 추가된다.

#[tauri::command]
fn ui_string(key: String) -> String {
    ai_note_core::ui_strings::s(&key, ai_note_core::ui_strings::Scope::App).to_string()
}

#[tauri::command]
fn app_version() -> String {
    ai_note_core::CORE_VERSION.to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ui_string, app_version])
        .run(tauri::generate_context!())
        .expect("앱을 시작하지 못했어요");
}
