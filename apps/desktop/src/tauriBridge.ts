// Tauri 런타임 브리지 어댑터 — 앱 패키지로 실행될 때만 설치된다.
// vitest(jsdom)에서는 __TAURI_INTERNALS__가 없어 undefined 유지 → 화면은
// 주입된 가짜 브리지로 검증된다.
type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

function invokeFactory(): Invoke | null {
  const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke: Invoke } })
    .__TAURI_INTERNALS__;
  return internals ? internals.invoke.bind(internals) : null;
}

export function installBridge(
  windowObj: Window & {
    __aiNoteBridge?: unknown;
    __aiNoteSettings?: unknown;
    __aiNoteWorkspace?: unknown;
    __aiNoteReview?: unknown;
    __aiNoteHistory?: unknown;
  }
) {
  const invoke = invokeFactory();
  if (!invoke) return;
  const typed = invoke as <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
  windowObj.__aiNoteBridge = {
    detectClaude: () => typed("detect_claude"),
    claudeManualSteps: () => typed("claude_manual_steps"),
    subscriptionState: () => typed("subscription_state"),
    subscriptionGuide: () => typed("subscription_guide"),
    connectInvitation: (invitation: string, passphrase: string) =>
      typed("connect_invitation", { invitation, passphrase }),
  };
  windowObj.__aiNoteReview = {
    inbox: () => typed("review_inbox"),
    diffOf: (id: string) => typed("review_diff", { id }),
    approve: (id: string) => typed("review_approve", { id }),
    reject: (id: string) => typed("review_reject", { id }),
  };
  windowObj.__aiNoteHistory = {
    history: () => typed("history_list"),
  };
  windowObj.__aiNoteWorkspace = {
    listDir: (dir: string) => typed("workspace_list", { dir }),
    readFile: (path: string) => typed("workspace_read", { path }),
    searchVault: (query: string) => typed("workspace_search", { query }),
    saveDocument: (path: string, content: string) =>
      typed("workspace_save", { path, content }),
  };
  windowObj.__aiNoteSettings = {
    accountDisplay: () => typed("account_display"),
    connectedRepo: () => typed("connected_repo"),
    appVersion: () => typed("app_version"),
    disconnectAccount: () => typed("disconnect_account"),
  };
}
