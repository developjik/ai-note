//! OS 샌드박스 시행 (M3 — L2, S6 실증 이식, §10.3).
//!
//! macOS: Seatbelt(sandbox-exec) 허용형 프로파일 — S6 실증 조합:
//! `(allow default)` + `(deny file-write*)` + 예외 subpath(~/.claude, 작업
//! 스크래치). claude(Bun 기반)가 살아남으면서 볼트 원본·키체인 쓰기 차단.
//! Windows: AppContainer/제한 토큰 — 동일 설계(쓰기 ACL 거부+예외 경로),
//! CI 매트릭스 동등 증명은 별도(계획 §10.3).
//!
//! L1(환경 스크럽)도 이 모듈이 시행한다: 자식은 최소 PATH만 받는다.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Seatbelt 프로파일 생성 — 작업 스크래치와 claude 홈(~/.claude)만 쓰기
/// 허용, 나머지 쓰기 전부 거부(정본 볼트·키체인 포함).
pub fn seatbelt_profile(workdir: &Path, extra_write_allows: &[PathBuf]) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    // Seatbelt subpath는 문자열 접두 매칭 — 심볼릭 링크 정규화 필수
    // (/var → /private/var 등). canonicalize 실패 시 원문 사용(부재 경로).
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let wd = canon(workdir);
    let tmp = canon(Path::new(&std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into())));
    let mut allows = format!(
        "(allow file-write* (subpath \"{home}/.claude\"))\n  \
         (allow file-write* (subpath \"{tmp}\"))\n  \
         (allow file-write* (subpath \"{workdir}\"))",
        home = home,
        tmp = tmp.display(),
        workdir = wd.display(),
    );
    for p in extra_write_allows {
        allows.push_str(&format!("\n  (allow file-write* (subpath \"{}\"))", canon(p).display()));
    }
    format!(
        "(version 1)\n  (allow default)\n  (deny file-write*)\n  {allows}\n"
    )
}

/// 샌드박스 실행 결과.
#[derive(Debug)]
pub struct SandboxedOutput {
    pub status: Option<std::process::ExitStatus>,
    pub stdout: String,
    pub stderr: String,
    /// 이 OS에서 샌드박스 미지원 → 호출자가 안전하게 거부할 수 있게.
    pub unsupported_platform: bool,
}

/// 명령을 샌드박스로 실행(macOS). L1: env_clear + PATH/HOME 최소 주입.
pub fn run_sandboxed(
    program: &str,
    args: &[String],
    workdir: &Path,
    extra_write_allows: &[PathBuf],
) -> SandboxedOutput {
    if !cfg!(target_os = "macos") {
        return SandboxedOutput {
            status: None,
            stdout: String::new(),
            stderr: "이 운영 체제의 안전 실행 장치는 아직 준비 중이에요".into(),
            unsupported_platform: true,
        };
    }
    let profile = seatbelt_profile(workdir, extra_write_allows);
    let out = Command::new("/usr/bin/sandbox-exec")
        .arg("-p")
        .arg(&profile)
        .arg(program)
        .args(args)
        .env_clear() // L1: 토큰·경로·환경 미전달
        .env("PATH", minimal_path())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .output();
    match out {
        Ok(o) => SandboxedOutput {
            status: Some(o.status),
            stdout: String::from_utf8_lossy(&o.stdout).to_string(),
            stderr: String::from_utf8_lossy(&o.stderr).to_string(),
            unsupported_platform: false,
        },
        Err(e) => SandboxedOutput {
            status: None,
            stdout: String::new(),
            stderr: format!("안전 실행 장치를 시작하지 못했어요: {e}"),
            unsupported_platform: false,
        },
    }
}

/// 최소 PATH — claude가 필요로 하는 표준 위치만(시스템 git 제외 불가피:
/// 읽기 경로이며 쓰기는 프로파일이 차단. P3는 '앱 의존' 금지 조항).
fn minimal_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/.local/bin:/usr/local/bin:/usr/bin:/bin:/opt/homebrew/bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 프로파일 형상 — 허용형+쓰기 거부+예외 subpath(canonical 경로).
    #[test]
    #[cfg(target_os = "macos")]
    fn m3_profile_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let p = seatbelt_profile(tmp.path(), &[]);
        assert!(p.contains("(allow default)"));
        assert!(p.contains("(deny file-write*)"));
        let canon = tmp.path().canonicalize().unwrap();
        assert!(p.contains(&format!("(subpath \"{}\")", canon.display())));
        assert!(p.contains(".claude"));
    }

    /// §10.3 (a)(b)(c) 결정론적 가짜 데몬 — 실제 OS 차단 단언(macOS).
    /// 가짜 데몬 = /bin/sh -c '쓰기 시도'. 보호 대상(볼트 역할)은 프로덕션
    /// 배치와 동일하게 샌드박스 예외 밖(HOME 아래 테스트 전용 폴더)에 둔다 —
    /// TMPDIR 예외가 임시루트 전체를 허용하는 것과 무관하게 '예외 밖 경로
    /// 쓰기 차단'을 증명한다(테스트 후 자동 정리).
    #[test]
    #[cfg(target_os = "macos")]
    fn m3_l2_forbidden_writes_blocked_allowed_writes_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let scratch = tmp.path().join("scratch");
        std::fs::create_dir_all(&scratch).unwrap();

        // 보호 대상: HOME 아래 (프로파일은 ~/.claude 외 HOME 쓰기 거부)
        let home = std::env::var("HOME").expect("HOME");
        let guard = crate::sandbox::tests::home_guard_dir("ainote-sb-test");
        std::fs::create_dir_all(&guard).unwrap();

        // (c) 파이프라인 밖 볼트 쓰기 → 차단
        let blocked = run_sandboxed(
            "/bin/sh",
            &[
                "-c".to_string(),
                format!("echo x > {}", guard.join("a.md").display()),
            ],
            &scratch,
            &[],
        );
        assert!(!blocked.unsupported_platform);
        assert!(
            !blocked.status.map(|s| s.success()).unwrap_or(false),
            "예외 밖 쓰기는 차단되어야 함 (stderr: {})",
            blocked.stderr
        );
        assert!(!guard.join("a.md").exists(), "파일이 생기면 안 됨");

        // 허용 경로(스크래치) → 성공
        let allowed = run_sandboxed(
            "/bin/sh",
            &[
                "-c".to_string(),
                format!("echo ok > {}", scratch.join("out.txt").display()),
            ],
            &scratch,
            &[],
        );
        assert!(
            allowed.status.map(|s| s.success()).unwrap_or(false),
            "스크래치 쓰기는 허용 (stderr: {})",
            allowed.stderr
        );
        assert!(scratch.join("out.txt").exists());
        let _ = std::fs::remove_dir_all(&guard);
        let _ = home; // HOME 참조 유지(가독)
    }

    /// HOME 아래 테스트 전용 디렉터리(자정리) — 외부 의존 없는 고유 이름.
    pub fn home_guard_dir(prefix: &str) -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let pid = std::process::id();
        PathBuf::from(home).join(format!(".{prefix}-{pid}"))
    }

    /// §10.3 (a) L1 — 샌드박스 자식에 토큰 환경변수 부재 단언.
    #[test]
    #[cfg(target_os = "macos")]
    fn m3_l1_environment_scrubbed_in_sandbox() {
        let tmp = tempfile::tempdir().unwrap();
        let scratch = tmp.path().join("s");
        std::fs::create_dir_all(&scratch).unwrap();
        // 부모에 토큰을 실제로 심어 env_clear 효력을 증명(F5 — 시딩 없으면
        // env_clear와 무관하게 통과하는 허술한 테스트가 됨). 즉시 제거.
        std::env::set_var("GITHUB_TOKEN", "ghp_should_not_leak");
        let probe = run_sandboxed(
            "/bin/sh",
            &["-c".to_string(), "env | grep -c GITHUB_TOKEN || true".to_string()],
            &scratch,
            &[],
        );
        std::env::remove_var("GITHUB_TOKEN");
        assert_eq!(probe.stdout.trim(), "0", "자식에 토큰 환경 없음(env_clear 효력)");
    }

    /// 비macOS에서는 명시적 미지원(안전 거부) — 시그니처 호환.
    #[test]
    #[cfg(not(target_os = "macos"))]
    fn m3_unsupported_platform_safe_reject() {
        let tmp = tempfile::tempdir().unwrap();
        let out = run_sandboxed("x", &[], tmp.path(), &[]);
        assert!(out.unsupported_platform);
    }
}
