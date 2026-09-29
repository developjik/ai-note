// 가상 렌더 단위 테스트 — M5(3,000행 중 표시 창만 렌더).
import { describe, expect, it } from "vitest";
import { render, fireEvent } from "@testing-library/react";
import { VirtualList, ROW_HEIGHT } from "./VirtualList";

const many = Array.from({ length: 3000 }, (_, i) => `문서-${i}`);

describe("트리 가상 렌더 (M5)", () => {
  it("3,000행 — DOM 노드는 표시 창(+오버스캔)만", () => {
    const { container } = render(
      <VirtualList items={many} renderItem={(s) => <span>{s}</span>} />
    );
    const rows = container.querySelectorAll("div > div > div > div");
    expect(rows.length).toBeLessThan(60); // 창(≈14)+오버스캔(16) 수준
  });

  it("스크롤 하단 — 마지막 문서 렌더", () => {
    const { container } = render(<VirtualList items={many} renderItem={(s) => <span>{s}</span>} />);
    const scroller = container.querySelector('div[style*="overflow-y"]') as HTMLElement;
    // onScroll 핸들러가 scrollTop 상태 갱신 → 슬라이스 이동
    fireEvent.scroll(scroller as HTMLElement, { target: { scrollTop: (many.length - 1) * ROW_HEIGHT } });
    // 3000행의 마지막 근처 문서가 DOM에 존재(문서-2999 또는 근접)
    const rendered = (scroller as HTMLElement).textContent ?? "";
    expect(rendered).toContain("문서-2999");
  });
});
