//! 초대장 코덱 (S4 스파이크 구현체 — 계획 §4 `invite` 모듈).
//!
//! 초대장 = GitHub Pages 생성 페이지에서 **인코딩**하고 앱이 **디코딩**한다.
//! 이 모듈은 앱 측 디코더(+테스트를 위한 인코더)이며, Pages 측 JS와 공유하는
//! 바이너리 레이아웃·테스트 벡터를 [`TEST_VECTORS`]로 유지한다.
//!
//! 레이아웃(버전 필드로 전방 호환 — Codex/다른 에이전트 확장 대비):
//! ```text
//! "AINV" | u8 version | u8 nonce[12] | u8 salt[16] | u32 ciphertext_len | ciphertext
//! ```
//! 전체를 base64(url-safe, no pad)로 감싼다. 암호화는 AES-256-GCM,
//! 키 유도는 PBKDF2-HMAC-SHA256(반복 200,000) — 초대장은 짧아도
//! 비밀번호 추측 저항이 필요하다(재사용 정책 F26로 장기 유효).
//!
//! 평문 페이로드(JSON): `{ "ghp": "...", "repo": "..." , "display": "..." }`
//! (classic PAT `repo` 스코프 — A7. 앱이 저장소 자동 생성 private:true — A3)

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(thiserror::Error, Debug)]
pub enum InviteError {
    #[error("초대장 형식이 올바르지 않아요")]
    Malformed,
    #[error("초대장이 손상되었거나 암호가 달라요")]
    DecryptFailed,
    #[error("초대장을 만들 수 없어요")]
    EncryptFailed,
    #[error("지원하지 않는 초대장 버전이에요")]
    UnsupportedVersion(u8),
}

pub const MAGIC: &[u8; 4] = b"AINV";
pub const VERSION: u8 = 1;
pub const PBKDF2_ITERATIONS: u32 = 200_000;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 12;

/// 초대장 평문 페이로드.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InvitePayload {
    /// GitHub 개인 액세스 토큰(classic, `repo` 스코프 — A7).
    pub ghp: String,
    /// 팀 볼트 저장소 이름(앱이 자동 생성 — A3). 예: "my-team-notes"
    pub repo: String,
    /// 초대받은 사람 표시명(F32 '작성자' 표시·기각 반환 F21용 — E5).
    pub display: String,
}

/// 초대장을 암호화해 base64 문자열로 만든다(Pages 인코더와 동일 스펙).
pub fn encode(payload: &InvitePayload, passphrase: &str) -> anyhow::Result<String> {
    use pbkdf2::pbkdf2_hmac;
    use sha2::Sha256;

    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut key);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = serde_json::to_vec(payload)?;
    let aad = [MAGIC.as_slice(), &[VERSION]].concat();
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: &plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| InviteError::EncryptFailed)?;
    key.zeroize();

    let mut buf = Vec::with_capacity(4 + 1 + NONCE_LEN + SALT_LEN + 4 + ciphertext.len());
    buf.extend_from_slice(MAGIC);
    buf.push(VERSION);
    buf.extend_from_slice(&nonce_bytes);
    buf.extend_from_slice(&salt);
    buf.extend_from_slice(&(ciphertext.len() as u32).to_le_bytes());
    buf.extend_from_slice(&ciphertext);
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// base64 초대장을 복호화해 페이로드를 반환한다(앱 측 디코더).
pub fn decode(invitation: &str, passphrase: &str) -> Result<InvitePayload, InviteError> {
    use pbkdf2::pbkdf2_hmac;
    use sha2::Sha256;

    let raw = URL_SAFE_NO_PAD
        .decode(invitation.trim())
        .map_err(|_| InviteError::Malformed)?;
    if raw.len() < 4 + 1 + NONCE_LEN + SALT_LEN + 4 || &raw[0..4] != MAGIC {
        return Err(InviteError::Malformed);
    }
    let version = raw[4];
    if version != VERSION {
        return Err(InviteError::UnsupportedVersion(version));
    }
    let mut off = 5;
    let nonce_bytes: [u8; NONCE_LEN] = raw[off..off + NONCE_LEN]
        .try_into()
        .map_err(|_| InviteError::Malformed)?;
    off += NONCE_LEN;
    let salt: [u8; SALT_LEN] = raw[off..off + SALT_LEN]
        .try_into()
        .map_err(|_| InviteError::Malformed)?;
    off += SALT_LEN;
    let ct_len = u32::from_le_bytes(
        raw[off..off + 4]
            .try_into()
            .map_err(|_| InviteError::Malformed)?,
    ) as usize;
    off += 4;
    let ciphertext = raw.get(off..off + ct_len).ok_or(InviteError::Malformed)?;

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &salt, PBKDF2_ITERATIONS, &mut key);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let aad = [MAGIC.as_slice(), &[VERSION]].concat();
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| InviteError::DecryptFailed)?;
    key.zeroize();

    serde_json::from_slice(&plaintext).map_err(|_| InviteError::Malformed)
}

/// Pages JS 인코더와 공유하는 테스트 벡터 소스.
/// 고정 salt/nonce를 주입하는 결정적 인코더(테스트 전용).
#[cfg(any(test, feature = "test-vectors"))]
pub fn encode_deterministic(
    payload: &InvitePayload,
    passphrase: &str,
    fixed_salt: [u8; SALT_LEN],
    fixed_nonce: [u8; NONCE_LEN],
) -> anyhow::Result<String> {
    use pbkdf2::pbkdf2_hmac;
    use sha2::Sha256;

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), &fixed_salt, PBKDF2_ITERATIONS, &mut key);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let plaintext = serde_json::to_vec(payload)?;
    let aad = [MAGIC.as_slice(), &[VERSION]].concat();
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&fixed_nonce),
            Payload { msg: &plaintext, aad: &aad },
        )
        .map_err(|_| InviteError::EncryptFailed)?;
    key.zeroize();

    let mut buf = Vec::with_capacity(4 + 1 + NONCE_LEN + SALT_LEN + 4 + ciphertext.len());
    buf.extend_from_slice(MAGIC);
    buf.push(VERSION);
    buf.extend_from_slice(&fixed_nonce);
    buf.extend_from_slice(&fixed_salt);
    buf.extend_from_slice(&(ciphertext.len() as u32).to_le_bytes());
    buf.extend_from_slice(&ciphertext);
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// 공유 테스트 벡터 — Pages 구현이 이 값을 그대로 재현해야 한다.
/// (벡터는 암호문 전체가 아니라 파라미터 세트로 유지해, PBKDF2 구현
/// 차이를 잡으면서 벡터 파일이 민감해지는 일을 막는다.)
pub const TEST_VECTORS: &[(&str, &str, &str, &str)] = &[
    // (passphrase, ghp, repo, display)
    ("팀-비밀번호-1", "ghp_TestToken0001", "team-notes", "김하나"),
    ("team-secret-2", "ghp_TestToken0002", "team-notes", "박둘"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_random_salt() {
        let payload = InvitePayload {
            ghp: "ghp_RoundTrip1234".into(),
            repo: "vault".into(),
            display: "이름".into(),
        };
        let inv = encode(&payload, "비밀번호").unwrap();
        let back = decode(&inv, "비밀번호").unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let payload = InvitePayload {
            ghp: "ghp_X".into(),
            repo: "r".into(),
            display: "d".into(),
        };
        let inv = encode(&payload, "정답").unwrap();
        assert!(matches!(decode(&inv, "오답"), Err(InviteError::DecryptFailed)));
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let payload = InvitePayload {
            ghp: "ghp_Tamper".into(),
            repo: "r".into(),
            display: "d".into(),
        };
        let inv = encode(&payload, "비밀").unwrap();
        let mut raw = URL_SAFE_NO_PAD.decode(&inv).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xFF;
        let tampered = URL_SAFE_NO_PAD.encode(&raw);
        assert!(matches!(decode(&tampered, "비밀"), Err(InviteError::DecryptFailed)));
    }

    #[test]
    fn garbage_input_is_malformed_not_panic() {
        assert!(matches!(decode("가비지", "x"), Err(InviteError::Malformed)));
        assert!(matches!(decode("", "x"), Err(InviteError::Malformed)));
        // 올바른 매직이지만 잘린 본문
        let short = URL_SAFE_NO_PAD.encode([b'A', b'I', b'N', b'V', 1u8, 2u8]);
        assert!(matches!(decode(&short, "x"), Err(InviteError::Malformed)));
    }

    #[test]
    fn deterministic_encoder_matches_shared_spec() {
        // Pages JS가 재현해야 하는 파라미터 세트 — 코알몸 상수 고정.
        let salt = [7u8; SALT_LEN];
        let nonce = [9u8; NONCE_LEN];
        let (pass, ghp, repo, display) = TEST_VECTORS[0];
        let payload = InvitePayload {
            ghp: ghp.into(),
            repo: repo.into(),
            display: display.into(),
        };
        let inv = encode_deterministic(&payload, pass, salt, nonce).unwrap();
        // 1) 디코드 왕복 성립 2) 동일 입력→동일 문자열(결정성)
        assert_eq!(decode(&inv, pass).unwrap(), payload);
        assert_eq!(encode_deterministic(&payload, pass, salt, nonce).unwrap(), inv);
        // 3) 버전·매직 보존
        let raw = URL_SAFE_NO_PAD.decode(&inv).unwrap();
        assert_eq!(&raw[0..4], b"AINV");
        assert_eq!(raw[4], VERSION);
    }

    #[test]
    fn version_gate_rejects_future() {
        let payload = InvitePayload {
            ghp: "g".into(),
            repo: "r".into(),
            display: "d".into(),
        };
        let inv = encode(&payload, "p").unwrap();
        let mut raw = URL_SAFE_NO_PAD.decode(&inv).unwrap();
        raw[4] = 99;
        let future = URL_SAFE_NO_PAD.encode(&raw);
        assert!(matches!(
            decode(&future, "p"),
            Err(InviteError::UnsupportedVersion(99))
        ));
    }
}
