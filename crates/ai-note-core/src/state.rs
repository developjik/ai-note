//! 로컬 앱 상태 파일 (M1 — 연결된 저장소 이름·표시명 보관).
//!
//! SQLite 전체 상태(M2+ changeset·알림·캐시)에 앞서, M1은 설정 화면이
//! 읽을 최소 상태만 JSON 파일로 보관한다. 온라인 전제(F19)로 오프라인
//! 대기열 없음. 계정 토큰은 여기 두지 않는다(keyring 전용, F14).

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppState {
    /// 연결된 팀 볼트 저장소 이름(없으면 미연결).
    pub connected_repo: Option<String>,
    /// 관리자(첫 사용자) 여부 — F16 혼합 모델 표시용.
    pub is_admin: bool,
}

pub fn state_path(app_data: &Path) -> std::path::PathBuf {
    app_data.join("app-state.json")
}

pub fn save(app_data: &Path, state: &AppState) -> Result<(), String> {
    std::fs::create_dir_all(app_data).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(state_path(app_data), json).map_err(|e| e.to_string())
}

pub fn load(app_data: &Path) -> AppState {
    std::fs::read_to_string(state_path(app_data))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m1_state_roundtrip_and_default() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(tmp.path()), AppState::default());
        let st = AppState {
            connected_repo: Some("team-notes".into()),
            is_admin: true,
        };
        save(tmp.path(), &st).unwrap();
        assert_eq!(load(tmp.path()), st);
        // 손상 파일 → 기본값 폴백(앱이 죽지 않게)
        std::fs::write(state_path(tmp.path()), "{깨진").unwrap();
        assert_eq!(load(tmp.path()), AppState::default());
    }
}
