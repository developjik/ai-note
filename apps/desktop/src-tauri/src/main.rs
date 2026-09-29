// AI Note 데스크톱 셸 진입점 — 실제 로직은 lib.rs(테스트 가능)에.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    ai_note_desktop_lib::run()
}
