//! git 연산 계층 — git2(libgit2) in-process (M1~M2 구현 — 계획 §4 `git`).
//!
//! 시스템 git 실행파일 의존 금지(P3). 단일 작성자 큐로 모든 git 쓰기를
//! 직렬화한다(D·S2 증명). ghp는 콜백 인증으로만 주입되며 환경변수·
//! 디스크 비기록(F14). 원격 연산은 octocrab/GitHub REST와 분업.
