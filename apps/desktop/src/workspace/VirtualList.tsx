// 트리 가상 렌더 — M5(3,000+ 문서 60fps 목표): 표시 창만 DOM에 렌더.
// 행 높이 고정 전제의 표준 윈도잉 — 스크롤 위치로 슬라이스 계산.
import { useRef, useState, type ReactNode } from "react";

export const ROW_HEIGHT = 34;
const OVERSCAN = 8;

export function VirtualList<T>({
  items,
  renderItem,
  height = 480,
  testId,
}: {
  items: T[];
  renderItem: (item: T, index: number) => ReactNode;
  height?: number;
  testId?: string;
}) {
  const [scrollTop, setScrollTop] = useState(0);
  const ref = useRef<HTMLDivElement | null>(null);

  const visibleCount = Math.ceil(height / ROW_HEIGHT) + OVERSCAN * 2;
  const first = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const slice = items.slice(first, first + visibleCount);

  return (
    <div
      ref={ref}
      data-testid={testId}
      onScroll={(e) => setScrollTop((e.target as HTMLDivElement).scrollTop)}
      style={{ height, overflowY: "auto" }}
    >
      <div style={{ height: items.length * ROW_HEIGHT, position: "relative" }}>
        <div
          style={{
            position: "absolute",
            top: first * ROW_HEIGHT,
            left: 0,
            right: 0,
          }}
        >
          {slice.map((item, i) => (
            <div key={first + i} style={{ height: ROW_HEIGHT }}>
              {renderItem(item, first + i)}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
