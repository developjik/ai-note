# D0 — 변경 세트 오케스트레이션 설계 (M2 착수 전 architect 서명 게이트물)

> 계획 §4.6/§14 ④: 본 문서 서명(architect CLEAR) 후 M2 착수. RALPLAN pass-2에서
> Architect CLEAR/APPROVE 시 지적된 F3(변경 세트 git 토폴로지·상태머신·오케스트레이션 소유)의
> 확정 설계다. 소스: 스펙 F6/F20/F21/F32, S2 증명, 계획 stage-02-revision §4.6.

## 1. 개념 — '변경 세트(changeset)'
모든 수정(직접 편집·AI 산출·충돌 해소)은 **변경 세트** 하나로 표현된다:
`Changeset { id, author_display, summary, base_commit, files[], origin: Edit|Agent|Resolution }`
변경 세트만이 반영 단위다. 원본 볼트는 읽기 전용 뷰이고, 작성은 항상
**작업 트리(전환 없음)** — `git_worktree`가 아니라 **인덱스 조작 + `cs/<id>` 브랜치 커밋**
(S2 교훈: 체크아웃 전환은 SAFE 전략 충돌을 낳는다. 무전환 = 충돌 원천 제거).

## 2. git 토폴로지 (S2 증명 기반)
```text
origin/main ──┐
              ├── cs/20260929-1430-김하나  (변경 세트 브랜치, 단일 커밋 스택)
              ├── cs/20260929-1432-에이전트
              └── ...
로컬 main: 원격 추적 전용(직접 커밋 금지 — F6). 커밋은 cs/<id>에만.
```
- **커밋 생성**: `git2` 인덱스에 파일 반영 → 트리 → `cs/<id>` 브랜치 커밋. S2 `GitWorker` 단일 소유 워커가 순차 수행(`!Send` 강제).
- **푸시+PR**: cs 브랜치 푸시 → GitHub PR 생성(승인 기록은 앱 DB, GitHub 네이티브 리뷰 불가 — 단일 신원).
- **반영**: 첫 승인 → GitHub 머지 API(원자성) → 로컬 main fetch → **수퍼셋 검사**.

## 3. 수퍼셋 누적과 비순차 승인 (F20)
- 승인 순서대로 반영한다. 대기 중 cs의 `base_commit`이 원격 main HEAD보다 뒤면:
  - **자동 수렴(3-way)**: libgit2 merge(S2 증명 루프) — 반영 충돌 없으면 자동 재작성 후 새 cs로 갱신.
  - **충돌 시**: AI 해소 작업(agent 모듈, 재시도 상한 3 → 초과 시 자동 취소+일상 언어 알림+재제출 유도, R7).
  - **이미 반영됨 판정**: 변경 내용이 이전 승인 머지에 완전 포함(동일 파일·동일 해시)되면 '이미 반영됨'(F32) — GitHub 머지 원자성이 이중 반영 차단.
- 비순차 승인 예: cs-B가 cs-A보다 먼저 승인되면 cs-A의 base는 뒤처짐 → 위 수렴 절차 동일 적용. 순서 강제 없음(F20 '승인 순 반영' = 승인된 것부터 즉시 반영).

## 4. 상태머신 (review 모듈 소유)
```text
draft ──제출──▶ pending_review ──첫 승인──▶ applied
                     │ 기각                     ▲
                     ▼                          │ (수렴 후 재승인)
                rejected ──작성자 재제출──▶ pending_review
                     └──AI 충돌 해소 중──▶ resolving ──성공──▶ pending_review(갱신)
                                            └──상한 초과──▶ cancelled+알림
```
- 전이는 SQLite(state 모듈)에 영속 — 앱 재시작 후 복원.
- `applied` 시 작성자 뷰에서 '검토 중' 초안 제거(A4), 이력 화면에 표시(F29).

## 5. 타 기기 동기화
- 알림 폴링(github 모듈, ETag)으로 원격 main 갱신 감지 → fetch → 볼트 뷰 갱신 + 검색 재색인(증분, S3).
- 다른 기기의 대기 cs는 리뷰 화면에만 표시(팀원은 반영 후 볼트 가시 — A4).

## 6. 오케스트레이션 소유 모듈
**`changeset` 모듈이 유일 소유**한다: 상태 전이·cs 브랜치 수명주기·수렴 트리거·취소.
`git_layer`(워커)·`github`(PR API)·`agent`(해소 작업)·`review`(판정)는 changeset이 지시하는
협력자다. M2 구현 순서: (1) changeset 상태머신+SQLite (2) cs 브랜치 파이프라인(S2 워커 통합)
(3) 자동 수렴 (4) wiremock PR 경합 시나리오(비순차·이미 반영됨).

## 7. 시나리오 매트릭스 (wiremock 테스트 목록 — M2 종료 기준)
| # | 시나리오 | 기대 |
|---|----------|------|
| 1 | 단일 cs 승인 | applied, 원격 main 반영 |
| 2 | 동일 cs 동시 승인 2회 | 1회만 applied, 2회째 '이미 반영됨'(F32) |
| 3 | cs-B 먼저 승인, cs-A base 낡음 | 자동 수렴 후 cs-A 재심사·반영 |
| 4 | 충돌 cs 승인 | resolving → AI 해소 → 갱신 cs 재승인 요청(R9 재확인 알림) |
| 5 | AI 해소 3회 실패 | cancelled + 일상 언어 알림 + 재제출 유도(R7) |
| 6 | 기각 → 재제출 | rejected → pending_review, 원문 유지(F21) |
| 7 | 앱 재시작 중간 상태 | SQLite에서 상태 복원, cs 브랜치 정합성 검사 |
