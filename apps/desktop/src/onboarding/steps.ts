// 온보딩 단계 기계 — M1 (E2E-1/2/3, F13/F16/F17/F26/F34).
// 순수 함수: 화면 없이 vitest로 검증한다. 화면은 이 상태를 구독해 렌더.
//
// 단계 흐름:
// invite(초대장 붙여넣기) → claude(설치 확인·하이브리드) → connect(연결)
// → done(완료). claude 단계는 Ready/Outdated/NotInstalled 하위 상태를 갖고
// 하이브리드 설치(auto → manual) 분기를 소유한다.

export type ClaudeState = "ready" | "outdated" | "notInstalled";
export type SubscriptionState = "loggedIn" | "needsLogin";

export type OnboardingStep = "invite" | "claude" | "subscription" | "connect" | "done";

export interface OnboardingState {
  step: OnboardingStep;
  claude: ClaudeState | null;
  subscription: SubscriptionState | null;
  autoInstallTried: boolean;
  connecting: boolean;
  result: "admin" | "member" | null;
  errorKey: string | null;
}

export const initialOnboarding: OnboardingState = {
  step: "invite",
  claude: null,
  subscription: null,
  autoInstallTried: false,
  connecting: false,
  result: null,
  errorKey: null,
};

export type Action =
  | { kind: "submit-invite" }
  | { kind: "claude-detected"; state: ClaudeState }
  | { kind: "subscription-detected"; state: SubscriptionState }
  | { kind: "subscription-recheck"; state: SubscriptionState }
  | { kind: "auto-install-failed" }
  | { kind: "manual-install-recheck"; state: ClaudeState }
  | { kind: "connect-start" }
  | { kind: "connect-done"; result: "admin" | "member" }
  | { kind: "connect-failed"; errorKey: string }
  | { kind: "retry" };

/** 단일 전이 — 순수. 화면/브리지는 이 함수로만 상태를 바꾼다. */
export function reduce(s: OnboardingState, a: Action): OnboardingState {
  switch (a.kind) {
    case "submit-invite":
      // 초대장 제출 → 설치 확인 단계로(F17 구독 로그인은 연결 단계에서)
      return { ...s, step: "claude", errorKey: null };
    case "claude-detected":
      return {
        ...s,
        claude: a.state,
        // 설치돼 있으면 구독 확인으로, 아니면 설치 안내에 머문다
        step: a.state === "ready" ? "subscription" : s.step,
        autoInstallTried: false,
      };
    case "subscription-detected":
      return {
        ...s,
        subscription: a.state,
        // 구독 로그인됨 → 연결로; 미로그인 → 구독 안내에 머문다(Q2 게이트)
        step: a.state === "loggedIn" ? "connect" : s.step,
      };
    case "subscription-recheck":
      return {
        ...s,
        subscription: a.state,
        step: a.state === "loggedIn" ? "connect" : s.step,
      };
    case "auto-install-failed":
      // 하이브리드(F34): 자동 실패 → 단계별 안내로 전환(재시도는 1회만)
      return { ...s, autoInstallTried: true };
    case "manual-install-recheck":
      return {
        ...s,
        claude: a.state,
        step: a.state === "ready" ? "subscription" : s.step,
      };
    case "connect-start":
      return { ...s, connecting: true, errorKey: null };
    case "connect-done":
      return { ...s, connecting: false, step: "done", result: a.result };
    case "connect-failed":
      return { ...s, connecting: false, errorKey: a.errorKey };
    case "retry":
      return { ...s, errorKey: null, step: "invite" };
  }
}

/** claude 단계가 안내 모드(수동 단계 표시)인지 — 하이브리드 2단계 판정. */
export function isManualGuideMode(s: OnboardingState): boolean {
  return (
    s.step === "claude" &&
    s.claude === "notInstalled" &&
    s.autoInstallTried
  );
}

/** 완료 화면 문구용 라벨 — 관리자/팀원 첫 화면 분기(F16). */
/** 구독 안내 모드 — 미로그인 사용자가 보는 안내(Q2 종결). */
export function isSubscriptionGuideMode(s: OnboardingState): boolean {
  return s.step === "subscription" && s.subscription === "needsLogin";
}

export function doneHeadline(s: OnboardingState): string {
  if (s.result === "admin") return "팀 노트가 준비됐어요";
  if (s.result === "member") return "팀 노트에 연결됐어요";
  return "";
}
