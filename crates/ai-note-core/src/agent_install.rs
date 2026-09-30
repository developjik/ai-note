//! claude CLI 설치 감지·하이브리드 설치 안내 (M1 — E2E-2, F34).
//!
//! 하이브리드 정책: 앱이 자동 설치를 먼저 시도하고(진행 표시), 실패하면
//! 복사 가능한 단계별 안내로 전환한다. 감지는 `claude --version` 실행으로
//! (PATH 탐색) — 시스템 git·Node 의존 없음(P3, claude만 외부 자식).

use std::process::Command;

/// 지원 하한 버전(감지된 버전이 이보다 오래면 안내 화면으로).
pub const MIN_SUPPORTED: (u32, u32) = (2, 0);

#[derive(Debug, Clone, PartialEq)]
pub enum InstallState {
    /// 설치됨 — 프로토콜 검증된 버전 범위(S1: 2.1.x 실측).
    Ready { version: String },
    /// 설치됐지만 버전이 너무 오래됨 — 업데이트 안내.
    Outdated { version: String },
    /// 미설치 — 하이브리드 설치 안내 필요.
    NotInstalled,
}

/// `claude --version` 출력에서 "X.Y.Z" 추출.
fn parse_version(output: &str) -> Option<String> {
    let mut best: Option<String> = None;
    for tok in output.split(|c: char| !c.is_ascii_digit() && c != '.') {
        let parts: Vec<&str> = tok.split('.').collect();
        if parts.len() >= 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
            // 가장 그럴듯한(2.x 형태) 토큰 선택
            if parts[0].parse::<u32>().map(|maj| maj >= 1).unwrap_or(false) {
                best = Some(tok.to_string());
            }
        }
    }
    best
}

/// 설치 상태 감지 — 현재 프로세스 PATH에서 claude 실행(없으면 빠른 실패).
pub fn detect() -> InstallState {
    detect_program("claude")
}

/// 감지 코어 — 프로그램 경로를 주입받는다. 테스트가 실제 설치 상태와 무관하게
/// 가짜 claude 실행파일로 감지·분류 동작을 검증할 수 있게 분리했다.
fn detect_program(claude: &str) -> InstallState {
    // 감지는 PATH·HOME만 유지한 최소 환경으로 실행한다(버전 확인은 비밀
    // 접근 불가). 에이전트 실구동(M3)은 전체 스크럽+S6 샌드박스를 적용.
    let out = Command::new(claude)
        .arg("--version")
        .env_clear()
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).to_string()
                + &String::from_utf8_lossy(&o.stderr);
            match parse_version(&text) {
                Some(v) => {
                    let maj: u32 = v.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let min: u32 = v.split('.').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                    if (maj, min) >= MIN_SUPPORTED {
                        InstallState::Ready { version: v }
                    } else {
                        InstallState::Outdated { version: v }
                    }
                }
                None => InstallState::Ready {
                    // 버전 문자열 파싱 실패는 '실행 가능'으로 보되 버전 미상
                    version: "unknown".into(),
                },
            }
        }
        _ => InstallState::NotInstalled,
    }
}

/// 자동 설치 시도(하이브리드 1단계) — npm 글로벌 설치를 앱이 대신 실행.
/// 성공 여부와 출력을 반환; 실패 시 안내 전환은 호출자(브리지)가 결정.
///
/// 주의: 이 명령은 Node/npm이 이미 있는 기기에서만 성공한다(없으면 실패 →
/// 단계별 안내로 전환 — 비개발자 기기 대부분의 경로). E2E-2의 하이브리드 분기.
/// 반환: (성공 여부, 명령 출력). 설치 도구 자체가 없으면 Err.
pub fn try_auto_install() -> anyhow::Result<(bool, String)> {
    let out = Command::new("npm")
        .args(["install", "-g", "@anthropic-ai/claude-code"])
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Ok((out.status.success(), text))
}

/// 단계별 안내 문구(하이브리드 2단계 — 복사 가능 명령 포함, F34).
/// git 어휘 없음(A1) — `npm install`은 설치 도구 명령으로 허용 페이지.
pub fn manual_guide_steps(os: &str) -> Vec<(String, String)> {
    match os {
        "macos" => vec![
            ("터미널 열기".into(), "응용 프로그램 → 유틸리티 → 터미널".into()),
            (
                "명령 붙여넣기".into(),
                "npm install -g @anthropic-ai/claude-code".into(),
            ),
            ("Enter 누르기".into(), "설치가 끝날 때까지 기다려 주세요".into()),
            (
                "확인하기".into(),
                "claude --version".into(),
            ),
        ],
        _ => vec![
            ("명령 프롬프트 열기".into(), "시작 버튼 → cmd 입력".into()),
            (
                "명령 붙여넣기".into(),
                "npm install -g @anthropic-ai/claude-code".into(),
            ),
            ("Enter 누르기".into(), "설치가 끝날 때까지 기다려 주세요".into()),
            (
                "확인하기".into(),
                "claude --version".into(),
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m1_version_parse() {
        assert_eq!(
            parse_version("2.1.270 (Claude Code)"),
            Some("2.1.270".into())
        );
        assert_eq!(parse_version("claude 2.3.1"), Some("2.3.1".into()));
        assert_eq!(parse_version("no version here"), None);
    }

    #[test]
    fn m1_version_gate_classifies() {
        let ready = InstallState::Ready { version: "2.1.270".into() };
        let outdated = InstallState::Outdated { version: "1.0.0".into() };
        assert_ne!(ready, outdated);
        assert_eq!(MIN_SUPPORTED, (2, 0));
    }

    /// 가짜 claude 실행파일 생성(플랫폼별 스크립트) — 감지 동작을 실제 설치
    /// 상태와 무관하게 검증한다(호스트 의존 테스트 금지 — CI 매트릭스 필수).
    fn fake_claude(dir: &std::path::Path, version_output: &str) -> String {
        let fake = dir.join(if cfg!(windows) { "claude.cmd" } else { "claude" });
        #[cfg(windows)]
        std::fs::write(&fake, format!("@echo {version_output}\r\n")).unwrap();
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(&fake, format!("#!/bin/sh\necho \"{version_output}\"\n")).unwrap();
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        fake.to_string_lossy().into_owned()
    }

    /// 설치된 claude(S1 실측 형태의 버전 출력) → Ready.
    #[test]
    fn m1_detect_installed_claude_is_ready() {
        let tmp = tempfile::tempdir().unwrap();
        match detect_program(&fake_claude(tmp.path(), "2.1.270 (Claude Code)")) {
            InstallState::Ready { version } => assert_eq!(version, "2.1.270"),
            other => panic!("설치됨으로 감지돼야 함: {other:?}"),
        }
    }

    /// 지원 하한 미만(1.x) → 구버전 안내 대상 분류.
    #[test]
    fn m1_detect_old_version_is_outdated() {
        let tmp = tempfile::tempdir().unwrap();
        match detect_program(&fake_claude(tmp.path(), "1.0.9")) {
            InstallState::Outdated { version } => assert_eq!(version, "1.0.9"),
            other => panic!("구버전으로 분류돼야 함: {other:?}"),
        }
    }

    /// 실행 불가 경로 → 미설치(설치 안내 경로). Windows v1은 AI 작업 제외
    /// (위협모델 §5)라 이 분류가 Windows CI 매트릭스의 유일한 감지 결과다.
    #[test]
    fn m1_detect_missing_claude_is_not_installed() {
        assert_eq!(
            detect_program("이-경로에-claude-없음"),
            InstallState::NotInstalled
        );
    }

    #[test]
    fn m1_manual_guide_has_no_git_vocabulary() {
        for os in ["macos", "windows"] {
            for (title, body) in manual_guide_steps(os) {
                assert!(
                    crate::ui_strings::audit_no_git_vocabulary(&title, crate::ui_strings::Scope::Allowed)
                        .is_empty()
                );
                assert!(
                    crate::ui_strings::audit_no_git_vocabulary(&body, crate::ui_strings::Scope::Allowed)
                        .is_empty(),
                    "안내 문구 어휘 위반: {body}"
                );
            }
        }
    }
}
