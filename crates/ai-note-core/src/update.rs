//! 무서명 업데이터 v1 (M5 — 계획 §4 `update`, S5 설계, P7 사용자 확정).
//!
//! 감지 → minisign 서명 검증 → 앱 내 안내 → 설정 버튼으로 수동 재다운로드
//! (자기 교체 없음). 다운로드 페이지가 공개키·체크섬을 게시한다.
//!
//! 흐름: GET /repos/{repo}/releases/latest → tag 파싱 → 현재 버전보다 높으면
//! 자산(checksum 파일)을 내려받아 minisign 분리 서명 검증 → UpdateNotice.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum UpdateCheck {
    /// 최신임 — 안내 없음.
    UpToDate { current: String },
    /// 새 버전 — 설정 화면 안내 + 수동 재다운로드 버튼(P7).
    Available {
        current: String,
        latest: String,
        /// 다운로드 페이지 주소(안내 버튼이 연다).
        download_url: String,
        /// 서명 검증 결과 요약(실패 시 안내에 표시하지 않음 — 다운로드 비권장).
        signature_verified: bool,
    },
    /// 확인 실패 — 일상 언어 안내(온라인 전제 F19).
    CheckFailed { reason: String },
}

/// 버전 비교 — "v0.9.2" 형태 점 정수 비교(프리릴리스 무시 v1 단순 모델).
pub fn parse_version(tag: &str) -> Option<(u64, u64, u64)> {
    let nums: Vec<u64> = tag
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.trim().parse().unwrap_or(0))
        .collect();
    match nums.len() {
        3 => Some((nums[0], nums[1], nums[2])),
        2 => Some((nums[0], nums[1], 0)),
        1 if !nums.is_empty() => Some((nums[0], 0, 0)),
        _ => None,
    }
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// 릴리스 JSON(REST /releases/latest 응답)에서 (tag, 다운로드 페이지) 추출.
pub fn parse_release(body: &str) -> Option<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = v.get("tag_name")?.as_str()?.to_string();
    let url = v
        .get("html_url")
        .and_then(|u| u.as_str())
        .unwrap_or_default()
        .to_string();
    Some((tag, url))
}

/// minisign 분리 서명 검증 — 공개키로 (체크섬 파일 내용, 서명) 대조.
/// minisign-verify 채택 지점(교체 필요시 여기만).
pub fn verify_minisign(public_key: &str, data: &str, signature: &str) -> bool {
    // 공개키는 순수 base64 또는 '주석+base64' 두 형태로 올 수 있다.
    let pk = minisign_verify::PublicKey::from_base64(public_key)
        .or_else(|_| minisign_verify::PublicKey::decode(public_key));
    let sig = match minisign_verify::Signature::decode(signature) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let pk = match pk {
        Ok(p) => p,
        Err(_) => return false,
    };
    pk.verify(data.as_bytes(), &sig, true).is_ok()
}

/// GitHub 릴리스 확인 전체 흐름(REST 클라이언트 주입 — wiremock 테스트).
pub async fn check_for_update(
    gh: &crate::github::GithubService,
    owner_repo: &str,
    current_version: &str,
    public_key: &str,
) -> UpdateCheck {
    let (status, body) = match gh
        .raw_get_status_body(&format!("/repos/{owner_repo}/releases/latest"))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return UpdateCheck::CheckFailed {
                reason: format!("새 버전 확인에 실패했어요: {e}"),
            }
        }
    };
    if !(200..300).contains(&status) {
        return UpdateCheck::CheckFailed {
            reason: "새 버전 정보를 가져오지 못했어요. 잠시 후 다시 시도해 주세요".into(),
        };
    }
    let Some((tag, url)) = parse_release(&body) else {
        return UpdateCheck::CheckFailed {
            reason: "새 버전 정보 형식이 달라요".into(),
        };
    };
    if !is_newer(&tag, current_version) {
        return UpdateCheck::UpToDate {
            current: current_version.to_string(),
        };
    }
    // 서명 검증 — 릴리스 자산(checksums.txt + .minisig)을 실제 다운로드
    // 경로에서 받아 검증(다운로드 페이지의 안내 파일과 동일 체인).
    let checksum = gh
        .release_asset(owner_repo, &tag, "checksums.txt")
        .await
        .unwrap_or_default();
    let sig = gh
        .release_asset(owner_repo, &tag, "checksums.txt.minisig")
        .await
        .unwrap_or_default();
    let verified = !checksum.is_empty() && verify_minisign(public_key, &checksum, &sig);
    UpdateCheck::Available {
        current: current_version.to_string(),
        latest: tag,
        download_url: url,
        signature_verified: verified,
    }
}

/// 사용자 안내 문구(설정 화면·일상 언어).
pub fn update_notice(check: &UpdateCheck) -> String {
    match check {
        UpdateCheck::UpToDate { .. } => "최신 버전를 쓰고 있어요".to_string(),
        UpdateCheck::Available { latest, signature_verified: true, .. } => format!(
            "새 버전({latest})이 나왔어요. 다운로드 페이지에서 새 설치 파일을 받아 덮어 설치해 주세요"
        ),
        UpdateCheck::Available { latest, .. } => format!(
            "새 버전({latest}) 안내를 받았지만 진짜인지 확인하지 못했어요. 다운로드 페이지의 안내를 따라 주세요"
        ),
        UpdateCheck::CheckFailed { reason } => reason.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m5_version_compare() {
        assert!(is_newer("v1.0.0", "v0.9.2"));
        assert!(!is_newer("v0.9.2", "v1.0.0"));
        assert!(!is_newer("v1.0.0", "v1.0.0"));
        assert!(is_newer("v1.0.1", "v1.0.0"));
        assert!(!is_newer("가비지", "v1.0.0"));
        assert_eq!(parse_version("v2.5.3"), Some((2, 5, 3)));
    }

    #[test]
    fn m5_release_parse() {
        let body = r#"{"tag_name":"v1.2.0","html_url":"https://github.com/t/r/releases/tag/v1.2.0"}"#;
        let (tag, url) = parse_release(body).unwrap();
        assert_eq!(tag, "v1.2.0");
        assert!(url.contains("releases/tag/v1.2.0"));
        assert!(parse_release("{깨진").is_none());
    }

    /// minisign 실증 — 공개 벡터(minisign-verify crate 테스트 벡터 재사용):
    /// 올바른 (키·데이터·서명)만 통과, 데이터 변조은 실패.
    #[test]
    fn m5_minisign_verify_positive_and_negative() {
        const PK: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        const SIG: &str = "untrusted comment: signature from minisign secret key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\ntrusted comment: timestamp:1555779966\tfile:test\nQtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==";
        assert!(verify_minisign(PK, "test", SIG), "정당 서명 통과");
        assert!(!verify_minisign(PK, "test-tampered", SIG), "변조 데이터 거부");
        assert!(!verify_minisign(PK, "test", "깨진서명"), "깨진 서명 거부");
        assert!(!verify_minisign("깨진키", "test", SIG), "깨진 키 거부");
    }

    #[test]
    fn m5_notice_everyday_language() {
        let ok = UpdateCheck::Available {
            current: "v0.9.0".into(),
            latest: "v1.0.0".into(),
            download_url: "u".into(),
            signature_verified: true,
        };
        let n = update_notice(&ok);
        assert!(n.contains("덮어 설치"));
        assert!(
            crate::ui_strings::audit_no_git_vocabulary(&n, crate::ui_strings::Scope::App)
                .is_empty()
        );
        let unverified = update_notice(&UpdateCheck::Available {
            current: "v0.9.0".into(),
            latest: "v1.0.0".into(),
            download_url: "u".into(),
            signature_verified: false,
        });
        assert!(unverified.contains("확인하지 못했어요"));
        assert!(update_notice(&UpdateCheck::UpToDate { current: "v1".into() }).contains("최신"));
    }
}
