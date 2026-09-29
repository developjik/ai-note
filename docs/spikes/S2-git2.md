# S2 — git2(libgit2) 클론/커밋/푸시·직렬화·3-way merge 증명

**상태: 검증 완료 (2026-09-29, git2 0.20.4 / libgit2 1.9)** — `cargo test -p spike-git2` 5/5 통과

## 증명 1 — 클론/커밋/푸시 roundtrip (`s2_1`)
- 로컬 bare 원격(file://) 대상 `Repository::clone` + 커밋 + `push` roundtrip 성공.
- **주의점 발견**: bare 원격의 HEAD가 git 기본 분기(master)를 가리키면 클론이 실패한다(`reference 'refs/heads/master' not found`). 앱은 원격 생성 직후 HEAD를 main으로 지정해야 한다 — M1 저장소 자동 생성(A3) 절차에 반영.
- push 인증은 `RemoteCallbacks` 자격증명 콜백으로 주입(로컬 file:// 테스트에서는 불필요). ghp는 콜백 클로저 안에서만 존재 — 환경변수·디스크 비기록(F14) 설계와 정합.

## 증명 2 — 동시 쓰기 직렬화 (`s2_2`, `s2_2b`)
### 핵심 발견: `git2::Repository`는 `!Send`다
libgit2 raw 포인터를 품은 저장소 핸들은 러스트 타입 시스템이 **스레드 간 이동 자체를 금지**한다(E0277 재현). 즉:
- '락으로 지키며 여러 태스크가 같은 저장소에 쓴다'는 설계는 **컴파일이 불가능**하다.
- **단일 작성자 원칙(계획 D)은 선택이 아니라 타입 시스템의 강제다.**

### 채택 모델: 소유 워커 스레드 + mpsc 채널
`GitWorker`(crates/spike-git2/src/lib.rs):
- 워커 스레드 하나가 `Repository`를 단독 소유, `GitJob`을 채널로 수신 순차 처리.
- 프로듀서(에디터 저장·AI 작업·충돌 해소)는 `submit()`만 호출.
- **15작업(3프로듀서×5) 처리 오류 0, 커밋 15개 전부 main에 존재 — 인덱스 락 경합 구조적으로 0회.**

### 구현 함정 기록 (M2 직접 반영)
- `Drop`에서 `join()`을 먼저 하면 송신자 드롭이 늦어져 `recv()`가 영구 대기한다(실제 교찰 재현·해결됨). 종료는 반드시 **`tx` 드롭 → `join()` 순서**(`GitWorker::finish()`).

## 증명 3 — 로컬 3-way merge 루프 (`s2_3`, `s2_3b`)
- base→양분기→같은 줄 수정: `repo.merge()` 후 `index.has_conflicts()` 감지 → 해소(재작성+add)→ **부 2개 머지 커밋** 완결.
- 서로 다른 줄 수정: 충돌 0, 자동 머지 커밋 완결.
- **함정 기록**: 분기 전환 체크아웃은 기본 SAFE 전략이 index 상태에 따라 `Conflict` 오류로 거부한다. 스파이크는 `CheckoutBuilder::force()`로 전환 확정. 앱의 실 토폴로지(cs/<id> 작업 트리)는 **작업 트리 무전환** 설계(D0)로 이 문제를 원천 회피한다.

## M4 설계 전제 충족
- PR 누적·승인 순 반영(F20)의 로컬 측 3-way 수렴 기제가 libgit2 merge로 성립.
- GitHub REST 측 PR 생성·동시 머지 원자성은 octocrab/wiremock(M1~M4)으로 별도 증명 — 본 스파이크는 로컬 git 계층 한정.

## 결론
git2 in-process 채택(계획 O1) 유지. 시스템 git 실행파일·번들 CLI 불요(P3).
