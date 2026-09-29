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
    pickPrompt: "왼쪽에서 문서를 선택해 주세요",
    previewMode: "미리보기",
    editMode: "편집하기",
    save: "저장",
    saving: "저장 중…",
    saved: "검토에 보냈어요",
  },
  review: { title: "검토함", approve: "반영하기", reject: "도로 돌리기" },
  history: { title: "지난 기록", applied: "반영됨", rejected: "도로 돌림" },
  settings: {
    title: "설정",
    accountLabel: "내 계정",
    vaultLabel: "팀 노트 저장소",
    versionLabel: "앱 버전",
    disconnect: "연결 끊기",
    disconnectHelp: "이 기기에서 계정 연결을 지워요. 팀 노트는 저장소에 그대로 남아요.",
    disconnected: "연결 없음",
  },
  onboard: {
    title: "시작하기",
    claudeInstall: "AI 도우미 깔기",
    invite: "초대장 붙여넣기",
    unsignedNotice:
      "처음 실행할 때 운영 체제가 '확인되지 않은 프로그램' 경고를 보여줄 수 있어요. 정상이며, OS별 안내는 설정 → 설치 안내에서 다시 볼 수 있어요.",
    inviteHelp:
      "팀 관리자에게 받은 초대장 문자열을 그대로 붙여넣어 주세요. 초대장과 함께 알려준 암호도 필요해요.",
    invitePlaceholder: "AINV로 시작하는 초대장 문자열을 붙여넣어 주세요",
    passphrasePlaceholder: "초대장 암호",
    inviteNext: "다음 단계로",
    claudeCheck: "AI 도우미(claude) 설치를 확인하고 있어요.",
    claudeChecking: "확인 중… 잠시만 기다려 주세요",
    subscriptionChecking: "Claude 구독 로그인을 확인하고 있어요…",
    subscriptionGuideTitle:
      "AI 도우미를 쓰려면 Claude 구독 로그인이 필요해요. 아래 단계를 따라 주세요.",
    subscriptionRecheck: "다시 확인",
    connectHelp: "모든 준비가 끝났어요. 팀 노트 저장소에 연결할게요.",
    connectNow: "연결하기",
    connecting: "연결 중…",
    doneHelp: "왼쪽 화면에서 바로 노트 작성을 시작할 수 있어요.",
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
