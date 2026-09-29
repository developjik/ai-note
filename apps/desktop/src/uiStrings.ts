// 사용자 표시 문자열 — M0 프런트 스캐폴딩 단계.
// M1에서 Tauri bridge로 Rust 코어의 ui_strings 단일 소스를 invoke해
// 이 모듈은 게이트웨이로 전환한다(한·영 git 어휘 감사 CI가 같은 소스를 검사).
export const ui = {
  nav: {
    workspace: "내 노트",
    review: "검토함",
    history: "지난 기록",
    settings: "설정",
  },
  workspace: {
    newDocument: "새 문서",
    searchPlaceholder: "문서 찾기 — 제목이나 내용으로",
  },
  review: { title: "검토함", approve: "반영하기", reject: "도로 돌리기" },
  history: { title: "지난 기록", applied: "반영됨", rejected: "도로 돌림" },
  onboard: {
    title: "시작하기",
    claudeInstall: "AI 도우미 깔기",
    invite: "초대장 붙여넣기",
  },
} as const;

// 금지 어휘 감사(프런트 번들용) — Rust 코어 ui_strings::DENYLIST_*와 동일 목록.
export const DENYLIST_KO = [
  "커밋", "푸시", "브랜치", "풀 리퀘스트", "풀리퀘스트", "머지", "리베이스",
  "클론", "체크아웃", "페치", "스테이징", "스태시",
];
export const DENYLIST_EN = [
  "commit", "push", "branch", "pull request", "pullrequest", "merge",
  "rebase", "clone", "checkout", "fetch", "revert", "stash",
];
export function auditNoGitVocabulary(text: string): string[] {
  const lower = text.toLowerCase();
  return [...DENYLIST_KO, ...DENYLIST_EN].filter((w) => lower.includes(w.toLowerCase()));
}
