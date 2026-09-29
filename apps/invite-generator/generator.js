// AI Note 초대장 생성기 — Rust 코덱(invite.rs)과 동일 와이어 형식.
//
// 형식: base64url(무패딩) of
//   "AINV"(4) ver(1)=0x01 nonce(12) salt(16) ct_len_le_u32(4) ciphertext+tag
// 키: PBKDF2-HMAC-SHA256(암호, salt, 200000, 256bit)
// 암호: AES-256-GCM(iv=nonce, aad="AINV"+ver)
// 브라우저 WebCrypto로 동작 — 입력이 외부로 나가지 않는다.
"use strict";

const MAGIC = [0x41, 0x49, 0x4e, 0x56]; // "AINV"
const VERSION = 1;
const PBKDF2_ITERATIONS = 200000;

/** 초대장 문자열 생성. 모든 입력은 문자열. */
async function makeInvitation({ ghp, repo, display, passphrase }) {
  const enc = new TextEncoder();
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const nonce = crypto.getRandomValues(new Uint8Array(12));

  const keyMat = await crypto.subtle.importKey("raw", enc.encode(passphrase), "PBKDF2", false, [
    "deriveKey",
  ]);
  const key = await crypto.subtle.deriveKey(
    { name: "PBKDF2", salt, iterations: PBKDF2_ITERATIONS, hash: "SHA-256" },
    keyMat,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt"]
  );

  const plaintext = enc.encode(JSON.stringify({ ghp, repo, display }));
  const aad = new Uint8Array([...MAGIC, VERSION]);
  const ct = new Uint8Array(
    await crypto.subtle.encrypt({ name: "AES-GCM", iv: nonce, additionalData: aad }, key, plaintext)
  );

  const len = new Uint8Array(4);
  new DataView(len.buffer).setUint32(0, ct.length, true);

  const buf = new Uint8Array(4 + 1 + 12 + 16 + 4 + ct.length);
  let o = 0;
  buf.set(MAGIC, o); o += 4;
  buf[o++] = VERSION;
  buf.set(nonce, o); o += 12;
  buf.set(salt, o); o += 16;
  buf.set(len, o); o += 4;
  buf.set(ct, o);

  return toBase64Url(buf);
}

/** 결정적 인코딩(공유 벡터 상호검증용 — 페이지에서는 사용 안 함). */
async function makeInvitationDeterministic({ ghp, repo, display, passphrase }, saltHex, nonceHex) {
  const salt = hexToBytes(saltHex);
  const nonce = hexToBytes(nonceHex);
  const enc = new TextEncoder();
  const keyMat = await crypto.subtle.importKey("raw", enc.encode(passphrase), "PBKDF2", false, [
    "deriveKey",
  ]);
  const key = await crypto.subtle.deriveKey(
    { name: "PBKDF2", salt, iterations: PBKDF2_ITERATIONS, hash: "SHA-256" },
    keyMat,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt"]
  );
  const plaintext = enc.encode(JSON.stringify({ ghp, repo, display }));
  const aad = new Uint8Array([...MAGIC, VERSION]);
  const ct = new Uint8Array(
    await crypto.subtle.encrypt({ name: "AES-GCM", iv: nonce, additionalData: aad }, key, plaintext)
  );
  const len = new Uint8Array(4);
  new DataView(len.buffer).setUint32(0, ct.length, true);
  const buf = new Uint8Array(4 + 1 + 12 + 16 + 4 + ct.length);
  let o = 0;
  buf.set(MAGIC, o); o += 4;
  buf[o++] = VERSION;
  buf.set(nonce, o); o += 12;
  buf.set(salt, o); o += 16;
  buf.set(len, o); o += 4;
  buf.set(ct, o);
  return toBase64Url(buf);
}

function toBase64Url(bytes) {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function hexToBytes(hex) {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

// 페이지 결선
if (typeof document !== "undefined" && document.getElementById("make")) {
  document.getElementById("make").addEventListener("click", async () => {
    const ghp = document.getElementById("pat").value.trim();
    const repo = document.getElementById("repo").value.trim();
    const display = document.getElementById("display").value.trim();
    const passphrase = document.getElementById("pass").value;
    const out = document.getElementById("out");
    if (!ghp || !repo || !display || !passphrase) {
      out.innerHTML = '<p class="warn">모든 칸을 채워 주세요.</p>';
      return;
    }
    out.innerHTML = "<p>만들고 있어요… (몇 초 걸려요)</p>";
    const invitation = await makeInvitation({ ghp, repo, display, passphrase });
    out.innerHTML =
      '<label>초대장 문자열 — 복사해서 팀원에게 보내주세요</label>' +
      '<textarea readonly onclick="this.select()">' + invitation + "</textarea>";
  });
}

// Node(vitest) 환경 노출
if (typeof module !== "undefined") {
  module.exports = { makeInvitation, makeInvitationDeterministic };
}
