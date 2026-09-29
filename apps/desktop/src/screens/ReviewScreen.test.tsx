// 검토함·이력 화면 테스트 — M4(E2E-6 화면 요소).
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, fireEvent } from "@testing-library/react";
import { ReviewScreen, actionNotice, type ReviewBridge } from "./ReviewScreen";
import { HistoryScreen, type HistoryBridge } from "./HistoryScreen";

afterEach(cleanup);

const cards = [
  {
    id: "cs-1",
    author_display: "김하나",
    summary: "회의록 고침",
    origin_label: "직접 수정",
    pr_number: 11,
    needs_reconfirmation: false,
    files: ["회의/a.md"],
  },
  {
    id: "cs-2",
    author_display: "에이전트",
    summary: "문서 정리",
    origin_label: "조정 후 재정리",
    pr_number: 12,
    needs_reconfirmation: true,
    files: ["회의/b.md"],
  },
];

function reviewBridge(): ReviewBridge & { approveMock: ReturnType<typeof vi.fn> } {
  const approveMock = vi.fn(async () => ({ kind: "applied" as const }));
  return {
    inbox: vi.fn(async () => cards),
    diffOf: vi.fn(async () => [
      { left: "오늘 ", right: "오늘 ", changed_left: false, changed_right: false },
      { left: "승인했다", right: "반려했다", changed_left: true, changed_right: true },
    ]),
    approve: approveMock,
    reject: vi.fn(async () => ({ kind: "rejected" as const })),
    approveMock,
  };
}

describe("검토함 화면 (M4)", () => {
  it("요약 카드 목록 — 작성자·기원 표시", async () => {
    render(<ReviewScreen bridge={reviewBridge()} />);
    expect(await screen.findByTestId("card-cs-1")).toBeTruthy();
    expect(screen.getByTestId("card-cs-1").textContent).toContain("김하나");
    expect(screen.getByTestId("card-cs-2").textContent).toContain("조정 후 재정리");
  });

  it("재확인 배지(R9) — 조정 건에만", async () => {
    render(<ReviewScreen bridge={reviewBridge()} />);
    await screen.findByTestId("review-cards");
    expect(screen.getByTestId("reconfirm-cs-2")).toBeTruthy();
    expect(screen.queryByTestId("reconfirm-cs-1")).toBeNull();
  });

  it("나란히 보기 — 바뀐 단어 하이라이트(mark)·공통은 평문", async () => {
    render(<ReviewScreen bridge={reviewBridge()} />);
    fireEvent.click(await screen.findByTestId("open-cs-1"));
    const view = await screen.findByTestId("diff-view");
    const marks = view.querySelectorAll("mark");
    expect(marks.length).toBeGreaterThanOrEqual(2); // 좌·우 각 1
    expect(view.textContent).toContain("반려했다");
  });

  it("승인 → '반영했어요' 안내 + 목록 갱신", async () => {
    const b = reviewBridge();
    render(<ReviewScreen bridge={b} />);
    fireEvent.click(await screen.findByTestId("open-cs-1"));
    fireEvent.click(await screen.findByTestId("approve-btn"));
    const notice = await screen.findByTestId("action-notice");
    expect(notice.textContent).toBe("반영했어요");
    expect(b.approveMock).toHaveBeenCalledWith("cs-1");
  });

  it("기각 안내 문구 — 일상 언어", async () => {
    render(<ReviewScreen bridge={reviewBridge()} />);
    fireEvent.click(await screen.findByTestId("open-cs-1"));
    fireEvent.click(await screen.findByTestId("reject-btn"));
    const notice = await screen.findByTestId("action-notice");
    expect(notice.textContent).toContain("도로 돌렸어요");
  });

  it("결과 안내 매핑 4종", () => {
    expect(actionNotice({ kind: "applied" })).toBe("반영했어요");
    expect(actionNotice({ kind: "already" })).toBe("이미 반영된 변경이에요");
    expect(actionNotice({ kind: "resolving" })).toContain("조정 중");
    expect(actionNotice({ kind: "rejected" })).toContain("도로 돌렸어요");
  });

  it("화면 문자열에 git·충돌 어휘 없음", async () => {
    const { container } = render(<ReviewScreen bridge={reviewBridge()} />);
    await screen.findByTestId("review-cards");
    const { auditNoGitVocabulary } = await import("../uiStrings");
    const text = container.textContent ?? "";
    expect(auditNoGitVocabulary(text)).toEqual([]);
    expect(!text.includes("충돌")).toBe(true);
  });
});

describe("지난 기록 화면 (M4)", () => {
  const rows = [
    {
      id: "h1",
      author_display: "김하나",
      summary: "회의록 고침",
      state_label: "반영됨",
      origin_label: "직접 수정",
      pr_number: 11,
    },
    {
      id: "h2",
      author_display: "박둘",
      summary: "문서 정리",
      state_label: "반영됨",
      origin_label: "조정 후 재정리",
      pr_number: 12,
    },
    {
      id: "h3",
      author_display: "에이전트",
      summary: "삭제 제안",
      state_label: "도로 돌림",
      origin_label: "AI 작업",
      pr_number: 13,
    },
  ];

  it("행 표시 — 작성자 표시명(F29/E5) + '조정 후 반영' 기원", async () => {
    const b: HistoryBridge = { history: vi.fn(async () => rows) };
    render(<HistoryScreen bridge={b} />);
    const list = await screen.findByTestId("history-rows");
    expect(list.children.length).toBe(3);
    expect(list.textContent).toContain("김하나");
    expect(list.textContent).toContain("조정 후 재정리");
    expect(list.textContent).toContain("도로 돌림");
  });

  it("빈 상태", async () => {
    const b: HistoryBridge = { history: vi.fn(async () => []) };
    render(<HistoryScreen bridge={b} />);
    expect(await screen.findByTestId("history-empty")).toBeTruthy();
  });
});
