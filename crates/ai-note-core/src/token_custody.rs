//! ghp 토큰 보관 (M1 구현 — 계획 §4 `token-custody`).
//!
//! OS 키체인(macOS Keychain/Windows Credential Manager) 보관·디스크 미기록.
//! 보장 범위는 main 무결성(F6/F14) — 자식 프로세스 기밀성은 3층 강제
//! 스택(S6: L1 CLI 권한 플래그→L2 OS 샌드박스→L3 notify 탐지)이 담당.
