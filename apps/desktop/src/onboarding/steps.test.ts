import { describe, expect, it } from "vitest";
import {
  initialOnboarding,
  reduce,
  isManualGuideMode,
  isSubscriptionGuideMode,
  doneHeadline,
  type OnboardingState,
} from "./steps";

describe("온보딩 단계 기계 (M1)", () => {
  it("초대장 제출 → claude 확인 단계", () => {
    const s = reduce(initialOnboarding, { kind: "submit-invite" });
    expect(s.step).toBe("claude");
  });

  it("설치돼 있으면(ready) 구독 확인 단계로", () => {
    const afterInvite = reduce(initialOnboarding, { kind: "submit-invite" });
    const s = reduce(afterInvite, { kind: "claude-detected", state: "ready" });
    expect(s.step).toBe("subscription");
    expect(s.claude).toBe("ready");
  });

  it("미설치면 claude 단계에 머문다(하이브리드 안내)", () => {
    const afterInvite = reduce(initialOnboarding, { kind: "submit-invite" });
    const s = reduce(afterInvite, { kind: "claude-detected", state: "notInstalled" });
    expect(s.step).toBe("claude");
    expect(isManualGuideMode(s)).toBe(false); // 아직 자동 시도 전
  });

  it("하이브리드: 자동 설치 실패 → 수동 안내 모드, 재확인 ready면 연결로", () => {
    let s: OnboardingState = reduce(initialOnboarding, { kind: "submit-invite" });
    s = reduce(s, { kind: "claude-detected", state: "notInstalled" });
    s = reduce(s, { kind: "auto-install-failed" });
    expect(isManualGuideMode(s)).toBe(true);
    s = reduce(s, { kind: "manual-install-recheck", state: "ready" });
    expect(s.step).toBe("subscription");
    expect(isManualGuideMode(s)).toBe(false);
  });

  it("오래된 버전은 연결로 넘어가지 않고 claude 단계 유지(업데이트 안내)", () => {
    const afterInvite = reduce(initialOnboarding, { kind: "submit-invite" });
    const s = reduce(afterInvite, { kind: "claude-detected", state: "outdated" });
    expect(s.step).toBe("claude");
  });

  it("구독 게이트(Q2): 로그인됨 → 연결, 미로그인 → 안내 유지 + 재확인 통과", () => {
    let s: OnboardingState = reduce(initialOnboarding, { kind: "submit-invite" });
    s = reduce(s, { kind: "claude-detected", state: "ready" });
    s = reduce(s, { kind: "subscription-detected", state: "needsLogin" });
    expect(s.step).toBe("subscription");
    expect(isSubscriptionGuideMode(s)).toBe(true);
    s = reduce(s, { kind: "subscription-recheck", state: "loggedIn" });
    expect(s.step).toBe("connect");
    expect(isSubscriptionGuideMode(s)).toBe(false);
  });

  it("연결 성공 — 관리자/팀원 결과 분기(F16)", () => {
    let s: OnboardingState = reduce(initialOnboarding, { kind: "submit-invite" });
    s = reduce(s, { kind: "claude-detected", state: "ready" });
    s = reduce(s, { kind: "subscription-detected", state: "loggedIn" });
    s = reduce(s, { kind: "connect-start" });
    expect(s.connecting).toBe(true);
    s = reduce(s, { kind: "connect-done", result: "admin" });
    expect(s.step).toBe("done");
    expect(doneHeadline(s)).toBe("팀 노트가 준비됐어요");
    s = reduce(s, { kind: "retry" });
    expect(s.step).toBe("invite");
  });

  it("연결 실패 → 오류 표시 후 다시 시도로 초대장 단계 복귀", () => {
    let s: OnboardingState = reduce(initialOnboarding, { kind: "submit-invite" });
    s = reduce(s, { kind: "claude-detected", state: "ready" });
    s = reduce(s, { kind: "subscription-detected", state: "loggedIn" });
    s = reduce(s, { kind: "connect-start" });
    s = reduce(s, { kind: "connect-failed", errorKey: "err.connect.network" });
    expect(s.step).toBe("connect");
    expect(s.connecting).toBe(false);
    expect(s.errorKey).toBe("err.connect.network");
    s = reduce(s, { kind: "retry" });
    expect(s.step).toBe("invite");
    expect(s.errorKey).toBeNull();
  });
});
