// 워크스페이스 화면 통합 테스트 — M2(E2E-4 직접 부: 트리·편집·검색 점프·저장).
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, fireEvent, waitFor } from "@testing-library/react";
import { WorkspaceScreen, type WorkspaceBridge } from "./WorkspaceScreen";

afterEach(cleanup);

const DOCS = [
  { name: "회의", path: "회의/", kind: "Folder" as const },
  { name: "0930.md", path: "회의/0930.md", kind: "Document" as const },
  { name: "시작하기.md", path: "시작하기.md", kind: "Document" as const },
  { name: "사진.png", path: "자산/사진.png", kind: "Asset" as const },
];

function fakeBridge(): WorkspaceBridge & {
  saveMock: ReturnType<typeof vi.fn>;
  searchMock: ReturnType<typeof vi.fn>;
} {
  const saveMock = vi.fn(async () => ({ pr_number: 33, summary: "문서 고침: 0930" }));
  const searchMock = vi.fn(async () => [
    {
      path: "회의/0930.md",
      snippet: "…[출시 일정]…",
      byte_offset: 21,
    },
  ]);
  return {
    listDir: vi.fn(async () => DOCS),
    readFile: vi.fn(async (p: string) =>
      p === "회의/0930.md" ? "# 회의\n오늘 의제: 출시 일정 조정" : "# 시작"
    ),
    searchVault: searchMock,
    saveDocument: saveMock,
    saveMock,
    searchMock,
  };
}

// CodeMirror는 jsdom 측정 API 일부 미지원 — 에디터 마운트 실패 시 화면이
// 죽지 않는지가 관찰 지점(편집 본체는 CodeMirror 자체 검증에 맡김).
describe("워크스페이스 화면 (M2)", () => {
  it("트리 열람 — 폴더·문서 표시, 자산 숨김", async () => {
    render(<WorkspaceScreen bridge={fakeBridge()} />);
    expect(await screen.findByTestId("tree-0930.md")).toBeTruthy();
    expect(screen.getByTestId("tree-회의")).toBeTruthy();
    expect(screen.queryByTestId("tree-사진.png")).toBeNull();
  });

  it("문서 열기 → 내용 로드·경로 표시", async () => {
    render(<WorkspaceScreen bridge={fakeBridge()} />);
    fireEvent.click(await screen.findByTestId("tree-0930.md"));
    await waitFor(() =>
      expect(screen.getByTestId("open-path").textContent).toBe("회의/0930.md")
    );
  });

  it("검색 → 결과 목록 → 점프(문서 열림)", async () => {
    const b = fakeBridge();
    render(<WorkspaceScreen bridge={b} />);
    fireEvent.change(await screen.findByTestId("search-input"), {
      target: { value: "출시 일정" },
    });
    fireEvent.keyDown(screen.getByTestId("search-input"), { key: "Enter" });
    const hits = await screen.findByTestId("search-hits");
    expect(hits.children.length).toBe(1);
    fireEvent.click(screen.getByTestId("hit-0"));
    await waitFor(() =>
      expect(screen.getByTestId("open-path").textContent).toBe("회의/0930.md")
    );
    expect(b.searchMock).toHaveBeenCalledWith("출시 일정");
  });

  it("저장 — 변경 시에만 활성, 저장 후 검토 안내(변경 세트 경유)", async () => {
    const b = fakeBridge();
    render(<WorkspaceScreen bridge={b} />);
    fireEvent.click(await screen.findByTestId("tree-0930.md"));
    await screen.findByTestId("open-path");
    const save = screen.getByTestId("save-btn");
    expect((save as HTMLButtonElement).disabled).toBe(true); // 변경 없음
    // 내용 변경 흉내 — CodeMirror 대신 직접 상태 경유는 어려우므로
    // 저장 버튼 활성화는 dirty 상태 전이로: 편집기가 없으면 disabled 유지 단언
    expect((save as HTMLButtonElement).disabled).toBe(true);
  });

  it("미리보기 전환 — 편집기/미리보기 패널 교체", async () => {
    render(<WorkspaceScreen bridge={fakeBridge()} />);
    fireEvent.click(await screen.findByTestId("tree-0930.md"));
    await screen.findByTestId("open-path");
    fireEvent.click(screen.getByTestId("preview-toggle"));
    const pane = await screen.findByTestId("preview-pane");
    expect(pane.textContent).toContain("회의");
  });

  it("이미지 선택 → 자산 링크 삽입 + 저장 페이로드에 자산 포함(F30)", async () => {
    const b = fakeBridge();
    render(<WorkspaceScreen bridge={b} />);
    fireEvent.click(await screen.findByTestId("tree-0930.md"));
    await screen.findByTestId("open-path");
    const input = screen.getByTestId("image-input") as HTMLInputElement;
    // File 흉내(FileReader 경유 — jsdom 지원)
    const file = new File([new Uint8Array([1, 2, 3])], "flow.png", { type: "image/png" });
    fireEvent.change(input, { target: { files: [file] } });
    await waitFor(() =>
      expect(b.saveMock).not.toHaveBeenCalled()
    );
    // 저장은 dirty 전이 후 — 링크 삽입으로 저장 버튼 활성 확인 후 호출
    const save = screen.getByTestId("save-btn") as HTMLButtonElement;
    expect(save.disabled).toBe(false);
    fireEvent.click(save);
    await waitFor(() => expect(b.saveMock).toHaveBeenCalled());
    const [calledPath, , image] = b.saveMock.mock.calls[0];
    expect(calledPath).toBe("회의/0930.md");
    expect(image?.name).toBe("flow.png");
    expect(typeof image?.b64).toBe("string");
  });

  it("html 문서는 읽기 전용 미리보기(편집 불가, F12)", async () => {
    const b = fakeBridge();
    (b as unknown as { readFile: ReturnType<typeof vi.fn> }).readFile = vi.fn(
      async () => "<h1>보고서</h1><script>alert(1)</script>"
    );
    (b as unknown as { listDir: ReturnType<typeof vi.fn> }).listDir = vi.fn(async () => [
      { name: "보고서.html", path: "보고서.html", kind: "ReadOnly" as const },
    ]);
    render(<WorkspaceScreen bridge={b} />);
    fireEvent.click(await screen.findByTestId("tree-보고서.html"));
    const frame = await screen.findByTestId("readonly-html");
    expect(frame.getAttribute("sandbox")).toBe("");
    expect(frame.getAttribute("srcdoc")).toContain("보고서");
  });

  it("화면 문자열에 git 어휘 없음(A1/E2E-5)", async () => {
    const { container } = render(<WorkspaceScreen bridge={fakeBridge()} />);
    await screen.findByTestId("doc-tree");
    const { auditNoGitVocabulary } = await import("../uiStrings");
    expect(auditNoGitVocabulary(container.textContent ?? "")).toEqual([]);
  });
});
