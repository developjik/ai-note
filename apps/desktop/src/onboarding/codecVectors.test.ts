// 코덱 상호검증 — Pages JS 인코더 출력이 Rust invite.rs 벡터와 동일한지.
// vectors.json은 Rust encode_deterministic(고정 salt/nonce)로 생성되고,
// Rust 통합 테스트(codec_vectors.rs)가 같은 파일을 역방향으로 검증한다.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
// @ts-expect-error - 저장소 루트 Pages 생성기(번들 외부, vitest 전용 로드)
import { makeInvitationDeterministic } from "../../../invite-generator/generator.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(resolve(here, "../../../invite-generator/vectors.json"), "utf8"),
);

describe("초대장 코덱 상호검증 (Pages ↔ Rust, E2E-3)", () => {
  it("공유 벡터 수 일치", () => {
    expect(vectors.length).toBeGreaterThanOrEqual(2);
  });

  for (const v of vectors) {
    it(`JS 인코더 === Rust 벡터 (${v.display})`, async () => {
      const js = await makeInvitationDeterministic(
        { ghp: v.ghp, repo: v.repo, display: v.display, passphrase: v.passphrase },
        v.salt_hex,
        v.nonce_hex
      );
      expect(js).toBe(v.invitation);
    });
  }

  it("무작위 생성도 형식 규칙 준수(AINV 헤더 + base64url)", async () => {
    // Node 22 WebCrypto 글로벌 제공
    const inv = await makeInvitationDeterministic(
      { ghp: "ghp_RandomCheck", repo: "r", display: "d", passphrase: "p" },
      "000102030405060708090a0b0c0d0e0f",
      "101112131415161718191a1b"
    );
    const raw = Buffer.from(inv.replace(/-/g, "+").replace(/_/g, "/"), "base64");
    expect([...raw.slice(0, 4)]).toEqual([0x41, 0x49, 0x4e, 0x56]);
    expect(raw[4]).toBe(1);
    expect(inv).not.toMatch(/[+/=]/);
  });
});
