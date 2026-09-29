# S1 — claude CLI 스트리밍 JSON 프로토콜 검증

**상태: 검증 완료 (2026-09-29, claude 2.1.270 / macOS arm64)**

## 검증 내용

### 호출 형식
```
claude --print --output-format stream-json --verbose [--permission-mode <mode>] "<prompt>"
```
- `--print`: 비대화 1회 실행 (데몬 작업 단위와 동일)
- `--output-format stream-json`: NDJSON 프레임 스트림
- `--verbose`: `result` 프레임 포함(비용·턴 수)
- `--permission-mode` 플래그 수용 확인 (기본 실행에서 승인됨)

### 프레임 인벤토리 (실측)
| 프레임 | subtype/내용 | 비고 |
|--------|--------------|------|
| `system` | `init` | `claude_code_version`, `session_id`, `tools`, `permissionMode`, `cwd`, `model`, `mcp_servers`, `skills` — **L1 CLI 권한 플래그 게이트 승격의 앵커** |
| `system` | `hook_started` / `hook_response` | 훅 이벤트 스트리밍 |
| `system` | `thinking_tokens` | 사고 토큰 델타 (진행률 표시 F28에 사용) |
| `assistant` | `message.content[]` | 블록 배열(`text` 등) — AI 채팅 패널 렌더 단위 |
| `result` | `subtype:"success"` | `is_error`, `duration_ms`, `num_turns`, `total_cost_usd`, **`result`(최종 텍스트)** |

실측 예: `{"type":"result","subtype":"success","is_error":false,"duration_ms":2767,"num_turns":1,"result":"OK"}`

## 어댑터 설계 결론 (M3 구현 지침)
1. **프로토콜 격리**: 어댑터는 `system/init`(버전·권한 확인) → `assistant`/`system` 스트림 → `result` 종결의 3단계 상태머신으로 파싱. 프레임 스키마 변경은 어댑터 뒤에 격리(R2).
2. **버전 감지 게이트**: `init.claude_code_version`을 읽어 지원 범위 밖이면 일상 언어 안내 + 업데이트 유도 (R2 완화).
3. **진행률**: `thinking_tokens` 델타 → 작업 진행 표시 (F28).
4. **세션**: `session_id` 프레임 필드로 재개(`--resume`) 가능 — 대화형 문서 작업에 사용.
5. **권한**: `--permission-mode` + `--allowedTools`류 플래그가 L1 강제 계층의 실체 (S6 참조 — CLI 자체 권한 플래그를 게이트 요구사항으로 승격).

## 관찰된 비고
- 이 개발기계의 claude는 커스텀 모델(glm-5.3) 구성 — `unrecognized_model` 경고가 stderr에 남지만 스트리밍·result에는 영향 없음. 앱 사용자 환경(구독 로그인)과 무관한 개발 환경 특성.
- 첫 호출에서 응답 지연 시 `thinking_tokens`만 길게 이어질 수 있음 — 어댑터는 타임아웃·취소(프로세스 kill)를 지원해야 함(M3).
