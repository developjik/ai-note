// Tauri 런타임 브리지 어댑터 — 앱 패키지로 실행될 때만 설치된다.
// vitest(jsdom)에서는 __TAURI_INTERNALS__가 없어 undefined 유지 → 화면은
// 주입된 가짜 브리지로 검증된다.
type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

function invokeFactory(): Invoke | null {
  const internals = (window as unknown as { __TAURI_INTERNALS__?: { invoke: Invoke } })
    .__TAURI_INTERNALS__;
  return internals ? internals.invoke.bind(internals) : null;
}

export function installBridge(windowObj: Window & { __aiNoteBridge?: unknown; __aiNoteSettings?: unknown }) {
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
  windowObj.__aiNoteSettings = {
    accountDisplay: () => typed("account_display"),
    connectedRepo: () => typed("connected_repo"),
    appVersion: () => typed("app_version"),
    disconnectAccount: () => typed("disconnect_account"),
  };
}
