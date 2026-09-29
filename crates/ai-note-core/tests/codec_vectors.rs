//! 공유 코덱 벡터 검증(통합) — apps/invite-generator/vectors.json의 각 벡터가
//! Rust 결정적 인코더 출력과 동일하고 디코드 왕복도 성립하는지.
//! Pages JS 인코더는 vitest(codecVectors.test.ts)에서 같은 파일로 검증한다.
//! 양방향이 같은 벡터를 공유 → 구현 드리프트 즉시 포착(E2E-3 근거).

#[cfg(feature = "test-vectors")]
#[test]
fn shared_vectors_match_rust_codec() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/invite-generator/vectors.json"
    );
    let raw = std::fs::read_to_string(path).expect("vectors.json 읽기");
    let vectors: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let arr = vectors.as_array().expect("벡터 배열");
    assert!(arr.len() >= 2, "최소 2개 벡터 필요");

    for v in arr {
        let pass = v["passphrase"].as_str().unwrap();
        let ghp = v["ghp"].as_str().unwrap();
        let repo = v["repo"].as_str().unwrap();
        let display = v["display"].as_str().unwrap();
        let salt_hex = v["salt_hex"].as_str().unwrap();
        let nonce_hex = v["nonce_hex"].as_str().unwrap();
        let expected = v["invitation"].as_str().unwrap();

        let payload = ai_note_core::invite::InvitePayload {
            ghp: ghp.into(),
            repo: repo.into(),
            display: display.into(),
        };
        let salt = hex::decode(salt_hex).unwrap();
        let nonce = hex::decode(nonce_hex).unwrap();
        let salt: [u8; 16] = salt.try_into().expect("salt 16바이트");
        let nonce: [u8; 12] = nonce.try_into().expect("nonce 12바이트");

        let produced =
            ai_note_core::invite::encode_deterministic(&payload, pass, salt, nonce).unwrap();
        assert_eq!(produced, expected, "벡터 불일치 — 코덱 드리프트");
        assert_eq!(
            ai_note_core::invite::decode(expected, pass).unwrap(),
            payload,
            "디코드 왕복 실패"
        );
    }
}
