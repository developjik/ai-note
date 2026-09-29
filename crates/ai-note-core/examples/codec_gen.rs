//! 일회용: 공유 코덱 벡터 JSON 생성(Pages↔Rust 상호검증용).
fn main() {
    let vectors = ai_note_core::invite::TEST_VECTORS;
    let mut out = Vec::new();
    for (i, (pass, ghp, repo, display)) in vectors.iter().enumerate() {
        let payload = ai_note_core::invite::InvitePayload {
            ghp: ghp.to_string(),
            repo: repo.to_string(),
            display: display.to_string(),
        };
        // 고정 salt/nonce — 벡터 결정성
        let mut salt = [0u8; 16];
        let mut nonce = [0u8; 12];
        for (j, b) in salt.iter_mut().enumerate() {
            *b = (i * 31 + j * 7 + 1) as u8;
        }
        for (j, b) in nonce.iter_mut().enumerate() {
            *b = (i * 17 + j * 13 + 5) as u8;
        }
        let inv = ai_note_core::invite::encode_deterministic(&payload, pass, salt, nonce).unwrap();
        out.push(serde_json::json!({
            "passphrase": pass, "ghp": ghp, "repo": repo, "display": display,
            "salt_hex": hex_of(&salt), "nonce_hex": hex_of(&nonce), "invitation": inv,
        }));
    }
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
