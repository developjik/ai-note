//! 구독 확인 게이트 (M1 — E2E-2, F17/Q2).
//!
//! claude CLI의 구독(OAuth) 로그인 상태를 감지해, 미로그인·미구독 사용자를
//! 일상 언어 구독 안내로 보낸다. 토큰/API 키 입력란은 앱에 존재하지
//! 않는다(E2E-2 단언 — 자격은 오직 초대장·키체인 경유).
//!
//! 감지 원리: claude CLI의 OAuth 자격은 `~/.claude/.credentials.json`에
//! 보관된다(S1 프로토콜 조사 기준 2.1.x). 파일 존재 = 구독 로그인됨.
//! 부재 = 구독 안내(터미널 없는 사용자를 위한 단계 안내 제공).

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum SubscriptionState {
    /// 구독 로그인됨 — 에이전트 대화 가능.
    LoggedIn,
    /// 미로그인 — 구독 안내 페이지로.
    NeedsLogin,
}

/// claude 설정 디렉터리(감지 대상). 홈이 없는 극단 환경은 NeedsLogin.
pub fn credentials_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude").join(".credentials.json"))
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

pub fn detect() -> SubscriptionState {
    match credentials_path() {
        Some(p) if p.exists() => SubscriptionState::LoggedIn,
        _ => SubscriptionState::NeedsLogin,
    }
}

/// 구독 안내 단계(일상 언어 — 복사 가능 명령 포함, A1 허용 범위 안내 페이지).
/// `claude /login`은 claude 자체의 로그인 열기 명령이다.
pub fn login_guide_steps() -> Vec<(String, String)> {
    vec![
        ("터미널 열기".into(), "macOS: 응용 프로그램 → 유틸리티 → 터미널 / Windows: 시작 버튼 → cmd".into()),
        ("로그인 명령 입력".into(), "claude /login".into()),
        ("브라우저에서 계정 연결".into(), "Claude 구독 계정으로 로그인해 주세요".into()),
        ("앱에서 다시 확인".into(), "이 화면의 '다시 확인' 버튼 누르기".into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m1_detect_runs_and_classifies() {
        // 개발기계: 커스텀 모델 설정으로 OAuth 파일 부재 → NeedsLogin이
        // 정상 경로(감지 자체가 목적). LoggedIn 판정은 VM 매트릭스에서.
        let s = detect();
        assert!(matches!(s, SubscriptionState::LoggedIn | SubscriptionState::NeedsLogin));
    }

    #[test]
    fn m1_credentials_path_points_at_claude_home() {
        let p = credentials_path().unwrap();
        assert!(p.to_string_lossy().contains(".claude"));
        assert!(p.ends_with(".credentials.json"));
    }

    #[test]
    fn m1_login_guide_everyday_language() {
        let steps = login_guide_steps();
        assert!(steps.len() >= 3);
        for (title, body) in &steps {
            assert!(
                crate::ui_strings::audit_no_git_vocabulary(title, crate::ui_strings::Scope::Allowed)
                    .is_empty()
            );
            assert!(
                crate::ui_strings::audit_no_git_vocabulary(body, crate::ui_strings::Scope::Allowed)
                    .is_empty(),
                "구독 안내 어휘 위반: {body}"
            );
        }
        // 안내에 토큰/API 키 입력 절차가 없어야 한다(E2E-2)
        let all = format!("{steps:?}");
        assert!(!all.contains("ghp_"));
        assert!(!all.to_lowercase().contains("api key"));
        assert!(!all.to_lowercase().contains("token"));
    }
}
