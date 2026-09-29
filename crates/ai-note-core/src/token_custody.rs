//! ghp 토큰 보관 (M1 구현 — 계획 §4 `token-custody`, F14).
//!
//! OS 키체인(macOS Keychain / Windows Credential Manager — keyring 크레이트)에
//! 보관하고 디스크 평문 기록을 하지 않는다. 토큰은 이 보관소에서만 꺼내
//! github/git 계층에 **일시적으로** 전달된다(환경변수·자식 프로세스 비노출).
//!
//! 보장 범위: main 무결성(토큰 소유=앱 단독). 동일 사용자 권한의 기밀성은
//! 3층 강제 스택(S6)이 담당하며 위협 모델 문서에 명시된다.

use keyring::Entry;

pub const SERVICE: &str = "com.ainote.desktop";
pub const PAT_KEY: &str = "team-ghp";
/// 초대장에 포함된 표시명(작성자 표시 F32·기각 반환 F21용)도 함께 보관한다.
pub const DISPLAY_KEY: &str = "display-name";

#[derive(thiserror::Error, Debug)]
pub enum CustodyError {
    #[error("보관소에 접근할 수 없어요")]
    Backend(String),
    #[error("보관된 계정 정보가 없어요")]
    NotFound,
}

fn entry(key: &str) -> Result<Entry, CustodyError> {
    Entry::new(SERVICE, key).map_err(|e| CustodyError::Backend(e.to_string()))
}

/// ghp를 키체인에 보관한다(기존 값 덮어쓰기).
pub fn store_pat(pat: &str) -> Result<(), CustodyError> {
    entry(PAT_KEY)
        .and_then(|e| e.set_password(pat).map_err(|e| CustodyError::Backend(e.to_string())))
}

/// 보관된 ghp를 읽는다. 없으면 NotFound.
pub fn load_pat() -> Result<String, CustodyError> {
    entry(PAT_KEY)
        .and_then(|e| e.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => CustodyError::NotFound,
            other => CustodyError::Backend(other.to_string()),
        }))
}

/// 표시명 보관(온보딩 시 캡처 — E5 계약).
pub fn store_display(name: &str) -> Result<(), CustodyError> {
    entry(DISPLAY_KEY)
        .and_then(|e| e.set_password(name).map_err(|e| CustodyError::Backend(e.to_string())))
}

pub fn load_display() -> Result<String, CustodyError> {
    entry(DISPLAY_KEY)
        .and_then(|e| e.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => CustodyError::NotFound,
            other => CustodyError::Backend(other.to_string()),
        }))
}

/// 연결 해제(팀 나가기·초대장 폐기) — 전체 비우기.
pub fn clear_all() -> Result<(), CustodyError> {
    for key in [PAT_KEY, DISPLAY_KEY] {
        if let Ok(e) = entry(key) {
            // 없는 항목 삭제는 성공으로 처리
            let _ = e.delete_credential();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 실제 OS 키체인 통합 테스트(개발기계·CI macos/windows runner에서 실행).
    // macOS: 자기 키체인 generic password 쓰기는 프롬프트 없이 동작한다.
    #[test]
    fn m1_keychain_roundtrip_and_clear() {
        clear_all().unwrap();
        assert!(matches!(load_pat(), Err(CustodyError::NotFound)));

        store_pat("ghp_roundtrip_test").unwrap();
        assert_eq!(load_pat().unwrap(), "ghp_roundtrip_test");

        store_display("김하나").unwrap();
        assert_eq!(load_display().unwrap(), "김하나");

        // 덮어쓰기(초대장 재사용 F26 — 새 팀 연결)
        store_pat("ghp_second").unwrap();
        assert_eq!(load_pat().unwrap(), "ghp_second");

        clear_all().unwrap();
        assert!(matches!(load_pat(), Err(CustodyError::NotFound)));
        assert!(matches!(load_display(), Err(CustodyError::NotFound)));
    }
}
