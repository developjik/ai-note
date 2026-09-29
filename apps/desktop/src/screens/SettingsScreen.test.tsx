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

// M5 업데이터 안내 — 설정 화면 분기(verified/unverified).
describe("설정 화면 업데이트 안내 (M5)", () => {
  const base = () => ({
    accountDisplay: async () => "팀장",
    connectedRepo: async () => "team/notes",
    appVersion: async () => "0.9.2",
    disconnectAccount: async () => {},
  });

  it("새 버전 있음 — 안내+다운로드 링크", async () => {
    render(
      <SettingsScreen
        bridge={{ ...base(), checkUpdate: async () => ({
          has_update: true,
          notice: "새 버전(v1.0.0)이 나왔어요. 다운로드 페이지에서 새 설치 파일을 받아 덮어 설치해 주세요",
          download_url: "https://github.com/t/r/releases/tag/v1.0.0",
        }) }}
      />
    );
    const notice = await screen.findByTestId("update-notice");
    expect(notice.textContent).toContain("덮어 설치");
    expect(screen.getByTestId("update-download").getAttribute("href")).toContain("v1.0.0");
  });

  it("검증 실패 안내 — 다운로드 비권장 문구(링크는 유지)", async () => {
    render(
      <SettingsScreen
        bridge={{ ...base(), checkUpdate: async () => ({
          has_update: true,
          notice: "새 버전(v1.0.0) 안내를 받았지만 진짜인지 확인하지 못했어요. 다운로드 페이지의 안내를 따라 주세요",
          download_url: "u",
        }) }}
      />
    );
    const notice = await screen.findByTestId("update-notice");
    expect(notice.textContent).toContain("확인하지 못했어요");
  });

  it("최신 버전 — 안내만, 링크 없음", async () => {
    render(
      <SettingsScreen
        bridge={{ ...base(), checkUpdate: async () => ({
          has_update: false,
          notice: "최신 버전를 쓰고 있어요",
          download_url: "",
        }) }}
      />
    );
    const notice = await screen.findByTestId("update-notice");
    expect(notice.textContent).toContain("최신");
    expect(screen.queryByTestId("update-download")).toBeNull();
  });
});
