// 온보딩 화면 요소 자동 테스트 — E2E-2 단언(토큰/API 키 입력란 부재).
// 하이브리드 설치 안내·구독 게이트 렌더 경로도 커버.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, fireEvent } from "@testing-library/react";

/** 초대장 단계를 통과시켜 claude/구독 감지가 돌게 만든다. */
async function passInviteStep() {
  fireEvent.change(screen.getByTestId("invite-input"), {
    target: { value: "AINV더미초대장" },
  });
  fireEvent.change(screen.getByTestId("passphrase-input"), {
    target: { value: "팀-암호" },
  });
  fireEvent.click(screen.getByTestId("submit-invite"));
}
import { OnboardScreen, type Bridge } from "./OnboardScreen";

afterEach(cleanup);

const notInstalledBridge = (): Bridge => ({
  detectClaude: vi.fn(async () => ({ state: "notInstalled" as const, version: "" })),
  claudeManualSteps: vi.fn(async () => [
    ["터미널 열기", "응용 프로그램 → 유틸리티 → 터미널"],
    ["명령 붙여넣기", "npm install -g @anthropic-ai/claude-code"],
  ] as [string, string][]),
  subscriptionState: vi.fn(async () => "loggedIn" as const),
  subscriptionGuide: vi.fn(async () => [] as [string, string][]),
  connectInvitation: vi.fn(async () => ({ result: "admin" as const, repo: "team-notes" })),
});

describe("온보딩 화면 (M1, E2E-2)", () => {
  it("초기 화면: 초대장·암호 입력만 — 토큰/API 키 입력란 부재", () => {
    const { container } = render(<OnboardScreen bridge={notInstalledBridge()} />);
    // 입력 요소는 초대장 textarea + 암호 input 단 2개
    expect(container.querySelectorAll("textarea").length).toBe(1);
    const inputs = container.querySelectorAll("input");
    expect(inputs.length).toBe(1);
    expect(inputs[0].getAttribute("type")).toBe("password");
    const html = container.innerHTML.toLowerCase();
    expect(html).not.toContain("ghp_");
    expect(html).not.toContain("api key");
    expect(html).not.toContain("token");
  });

  it("미설치 → 하이브리드 수동 안내 단계 표시", async () => {
    render(<OnboardScreen bridge={notInstalledBridge()} />);
    await passInviteStep();
    const steps = await screen.findByTestId("manual-steps");
    expect(steps.textContent).toContain("npm install -g @anthropic-ai/claude-code");
  });

  it("구독 미로그인 → 구독 안내 + 다시 확인 버튼(Q2)", async () => {
    const b = notInstalledBridge();
    b.claudeManualSteps = vi.fn(async () => [] as [string, string][]);
    b.detectClaude = vi.fn(async () => ({ state: "ready" as const, version: "2.1.270" }));
    b.subscriptionState = vi.fn(async () => "needsLogin" as const);
    b.subscriptionGuide = vi.fn(
      async () => [["로그인 명령 입력", "claude /login"]] as [string, string][]
    );
    render(<OnboardScreen bridge={b} />);
    await passInviteStep();
    const guide = await screen.findByTestId("subscription-steps");
    expect(guide.textContent).toContain("claude /login");
    expect(screen.getByTestId("subscription-recheck")).toBeTruthy();
    // 토큰/API 키 입력 요소는 구독 단계에서도 부재
    expect(document.querySelectorAll("input").length).toBe(0);
  });
});
