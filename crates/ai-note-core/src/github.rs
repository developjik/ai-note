//! GitHub REST 서비스 (M1~M4 구현 — 계획 §4 `github`).
//!
//! ghp 보유·저장소 자동 생성(private:true — A3/A7)·PR 생성·머지 원자성
//! (F32)·ETag 조건부 요청·지수 백오프·SQLite 캐시(R3).
//! 승인 상태는 앱이 관리한다(GitHub 네이티브 required-reviews 불가).
