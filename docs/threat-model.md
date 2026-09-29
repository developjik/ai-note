# AI Note 위협 모델 (M3 — architect 리뷰 게이트물)

> 계획 §10.3: 본 문서 + architect 리뷰 통과가 M3 종료 조건이다.
> 구조 검증의 보장 범위를 정직하게 규정한다(과잉 약속 금지).

## 1. 보호 자산과 보장 범위

| 자산 | 보장 | 근거(계층) |
|---|---|---|
| **원격 main 무결성** | 승인 없이는 에이전트가 main에 반영 불가 | 토큰 격리(앱만 보유) + GitHub 머지 원자성 + PR 파이프라인 단일 경로(F6) |
| **볼트 원본(로컬)** | 에이전트 직접 쓰기 차단 | L2 Seatbelt `deny file-write*`(예외: 스크래치·`~/.claude`·TMPDIR) |
| **자격 증명(PAT)** | 에이전트 접근 불가 | L1 환경 스크럽 + 키체인 경로 쓰기·읽기 차단(L2 프로파일은 쓰기 거부, 읽기 제한 아님 — §3 참조) |
| **기밀성(볼트 내용 열람)** | **범위 외** | 로컬 동일 사용자 권한으로 실행되는 프로세스의 파일 읽기는 OS 수준에서 허용됨. 아래 §3 참조 |

## 2. 강제 스택(L1·L2·L3) 구현 상태

- **L1 — 환경 스크럽(자식 자기 권한 축소 게이트)**: `run_sandboxed`는 `env_clear()` 후 최소 PATH/HOME만 주입. 테스트 `m3_l1_environment_scrubbed_in_sandbox`가 자식 환경에 토큰 부재를 실측 단언. `--permission-mode` CLI 플래그 전달은 claude CLI 버전 확인 후 M3 종료 전 반영(변경 여지 명시).
- **L2 — OS 샌드박스**: macOS Seatbelt 허용형 프로파일(S6 실증 이식). 테스트 `m3_l2_forbidden_writes_blocked_allowed_writes_ok`가 (i) 예외 밖 경로 쓰기 차단 (ii) 스크래치 쓰기 허용을 실 OS 호출로 단언. **Windows AppContainer/제한 토큰은 미구현 — CI 매트릭스 동등 증명 이월(계획 §10.3), 이 문서의 Windows 주장은 설계 수준이다.**
- **L3 — 탐지**: `detect_vault_tampering`(mtime 스냅숏 대비 무단 변경 탐지 → `SecurityAlert` 이벤트 F25). 실시간 파일 이벤트 감시(FSEvents/ReadDirectoryChangesW)는 v2 — M3는 저장 시점 무결성 재확인으로 탐지.

## 3. 명시적 비보장(범위 외)

1. **로컬 기밀성**: 같은 사용자 계정의 다른 프로세스는 볼트 파일을 읽을 수 있다. 샌드박스 프로파일은 `file-write*`만 거부(전면 `deny default`는 claude Bun 런타임을 죽임 — S6 실증). 파일 읽기 보호는 전체 디스크 암호화·계정 분리 운영 정책 영역이다.
2. **사용자 자발적 권한 상승**: 사용자가 직접 토큰을 에이전트에게 주는 행위(프롬프트 인젝션에 대한 인간 방어선)는 기술적 차단 대상 아님 — 검토 게이트(F6)가 최종 방어선.
3. **GitHub 계정 자체 침해**: PAT 유출 시 원격 저장소 보호는 GitHub 플랫폼 영역. 앱은 키체인 보관·초기화 페이지 폐기 안내로 노출면 최소화.

## 4. 공격 시나리오와 대응(구조 검증 매트릭스 대응)

| 공격 | 차단 계층 | 검증 |
|---|---|---|
| 에이전트가 볼트 `.git` 조작으로 main 직접 반영 | L1(환경 무토큰)+L2(`.git` 미노출 — 스냅숏만) + 원격은 PAT 필요(앱만) | m3_l1/l2 테스트 + 구조: 샌드박스에 원본 저장소 경로 부재 |
| 에이전트가 키체인에서 PAT 탈취 | L1+키체인은 앱 자격 컨텍스트 | 토큰은 자식 환경·명령줄·디스크 비기록(코드 감사 지점) |
| 적대적 프롬프트로 도구 남용(파이프라인 밖 쓰기) | L2 쓰기 거부 + L3 탐지 알림 | m3_l2 차단 단언 + detect_vault_tampering |
| 이중 승인 레이스로 main 중복 반영 | GitHub 머지 원자성 | m2_scenario_2(AlreadyMerged→'이미 반영됨') |
| 무한 충돌 해소 반복(서비스 거부) | R7 상한 3회 → 자동 취소 + 알림 | m2_scenario_5 + m3_resolution_failure_surfaces_error |

## 5. 잔여 리스크와 완화 계획

- Windows L2 미구현 → v1 배포 전 AppContainer 매트릭스(M5 게이트에 포함 필수).
- 실시간 L3 미장착 → 저장 시점 재확인 + 워커 직렬화(GIT_OPS_LOCK)로 우회 쓰기 창 최소화.
- claude CLI 업데이트로 프로토콜/플래그 변경 → S1 어댑터 격리(claude_args 단일 지점) + 버전 게이트(agent_install MIN_SUPPORTED).
