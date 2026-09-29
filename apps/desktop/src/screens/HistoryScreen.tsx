// 지난 기록 화면 — M4(F29/E5): 반영/기각/취소 목록 + 작성자 표시명 +
// '조정 후 반영' 기원 표시(§4.9).
import { useEffect, useState } from "react";
import { ui } from "../uiStrings";

export interface HistoryRowDto {
  id: string;
  author_display: string;
  summary: string;
  state_label: string;
  origin_label: string;
  pr_number: number | null;
}

export interface HistoryBridge {
  history(): Promise<HistoryRowDto[]>;
}

declare global {
  interface Window { __aiNoteHistory?: HistoryBridge }
}

export function HistoryScreen({ bridge }: { bridge?: HistoryBridge }) {
  const b = bridge ?? window.__aiNoteHistory;
  const [rows, setRows] = useState<HistoryRowDto[]>([]);

  useEffect(() => {
    if (!b) return;
    b.history().then(setRows).catch(() => setRows([]));
  }, [b]);

  return (
    <section aria-label={ui.history.title}>
      <h2>{ui.history.title}</h2>
      {rows.length === 0 && <p data-testid="history-empty">{ui.history.empty}</p>}
      <ul data-testid="history-rows">
        {rows.map((r) => (
          <li key={r.id} data-testid={`row-${r.id}`}>
            <strong>{r.summary}</strong>
            <span>
              {r.author_display} · {r.state_label}
              {r.origin_label !== "직접 수정" && ` · ${r.origin_label}`}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
