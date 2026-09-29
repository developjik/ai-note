// 설정 화면 자동 테스트 — M1(계정 표시·연결 끊기·토큰 미표시).
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, fireEvent, waitFor } from "@testing-library/react";
import { SettingsScreen } from "./SettingsScreen";

afterEach(cleanup);

const fakeBridge = () => ({
  accountDisplay: vi.fn(async () => "김하나"),
  connectedRepo: vi.fn(async () => "team-notes"),
  appVersion: vi.fn(async () => "0.1.0"),
  disconnectAccount: vi.fn(async () => {}),
});

describe("설정 화면 (M1)", () => {
  it("계정 표시명·저장소 표시", async () => {
    render(<SettingsScreen bridge={fakeBridge()} />);
    const display = await screen.findByTestId("settings-display");
    expect(display.textContent).toBe("김하나");
    expect(screen.getByTestId("settings-repo").textContent).toBe("team-notes");
  });

  it("연결 끊기 → 계정 정보 사라짐", async () => {
    const b = fakeBridge();
    render(<SettingsScreen bridge={b} />);
    await screen.findByTestId("settings-display");
    fireEvent.click(screen.getByTestId("disconnect-btn"));
    await waitFor(() =>
      expect(screen.getByTestId("settings-display").textContent).toBe("연결 없음")
    );
    expect(b.disconnectAccount).toHaveBeenCalled();
  });

  it("화면에 토큰 원문·ghp 접두사·API 키 입력 요소가 없다(E2E-2)", async () => {
    const { container } = render(<SettingsScreen bridge={fakeBridge()} />);
    await screen.findByTestId("settings-display");
    const html = container.innerHTML.toLowerCase();
    expect(html).not.toContain("ghp_");
    expect(html).not.toContain("api key");
    expect(html).not.toContain("token");
    expect(container.querySelectorAll("input").length).toBe(0);
  });
});
