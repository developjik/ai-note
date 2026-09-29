//! AI Note (임시명) Rust 코어 — v1 모듈 11종 (계획 §4).
//!
//! 구조 원칙(계획 P1~P5): git 연산·ghp 토큰은 이 코어(=앱)만 수행하고
//! 에이전트(`agent`)는 샌드박스 작업사본만 다룬다. 모든 수정은 단일
//! 변경 세트→PR 파이프라인(`changeset`)을 통과한다(F6).
//!
//! 모듈 지도:
//! - [`vault`]        md 볼트 파일 계층 (F11 순수 표준 md, F30 이미지 자산)
//! - [`git_layer`]    git2(libgit2) in-process git 연산 — 단일 작성자 큐(D)
//! - [`github`]       GitHub REST(ghp 보유·PR 생성/머지·ETag 캐시)
//! - [`changeset`]    변경 세트 오케스트레이션 (D0 설계 구현체)
//! - [`agent`]        claude CLI 슈퍼바이저·샌드박스 작업사본·병렬 큐 (F28)
//! - [`invite`]       초대장 코덱 (S4 — aes-gcm+pbkdf2)
//! - [`review`]       리뷰 상태머신 — 첫 승인 반영·기각 재제출 (F21/F32)
//! - [`search`]       전문 검색(Tantivy 한국어 바이그램, F24)
//! - [`state`]        SQLite 로컬 상태 저장
//! - [`token_custody`]ghp 토큰 보관(OS 키체인) — 자식 프로세스 비노출 (F14)
//! - [`update`]       무서명 업데이터 — 감지→minisign 검증→수동 재다운로드 (S5)
//! - [`ui_strings`]   사용자 표시 문자열 단일 소스 + git 어휘 감사 (A1/R8)
//! - [`bridge`]       Tauri 프런트엔드 명령 브리지 (일상 언어 에러 변환)

pub mod ui_strings;
pub mod invite;
pub mod agent_install;
pub mod onboarding;
pub mod subscription;

pub mod vault;
pub mod git_layer;
pub mod github;
pub mod changeset;
pub mod agent;
pub mod sandbox;
pub mod review;
pub mod search;
pub mod state;
pub mod token_custody;
pub mod update;
pub mod bridge;

/// 코어 버전 — 앱 정보 화면·진단 리포트에 표시.
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
