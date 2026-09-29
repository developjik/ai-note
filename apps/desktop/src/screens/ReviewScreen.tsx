// 검토함 화면 — M4(E2E-6): 요약 카드 + 나란히 보기(단어 단위 하이라이트),
// 승인(반영)/기각, 재확인 배지(R9), '이미 반영됨' 안내(F32).
import { useEffect, useState } from "react";
import { ui } from "../uiStrings";

export interface ReviewCardDto {
  id: string;
  author_display: string;
  summary: string;
  origin_label: string;
  pr_number: number | null;
  needs_reconfirmation: boolean;
  files: string[];
}

export interface DiffChunkDto {
  left: string;
  right: string;
  changed_left: boolean;
  changed_right: boolean;
}

export type ReviewActionResult =
  | { kind: "applied" }
  | { kind: "already" }
  | { kind: "resolving" }
  | { kind: "rejected" };

export interface ReviewBridge {
  inbox(): Promise<ReviewCardDto[]>;
  diffOf(id: string): Promise<DiffChunkDto[]>;
  approve(id: string): Promise<ReviewActionResult>;
  reject(id: string): Promise<ReviewActionResult>;
}

declare global {
  interface Window { __aiNoteReview?: ReviewBridge }
}

export function actionNotice(r: ReviewActionResult): string {
  switch (r.kind) {
    case "applied":
      return ui.review.noticeApplied;
    case "already":
      return ui.review.noticeAlready;
    case "resolving":
      return ui.review.noticeResolving;
    case "rejected":
      return ui.review.noticeRejected;
  }
}

export function ReviewScreen({ bridge }: { bridge?: ReviewBridge }) {
  const b = bridge ?? window.__aiNoteReview;
  const [cards, setCards] = useState<ReviewCardDto[]>([]);
  const [openId, setOpenId] = useState<string | null>(null);
  const [chunks, setChunks] = useState<DiffChunkDto[]>([]);
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!b) return;
    b.inbox().then(setCards).catch(() => setCards([]));
  }, [b]);

  async function open(card: ReviewCardDto) {
    if (!b) return;
    setOpenId(card.id);
    setChunks(await b.diffOf(card.id));
    setNotice("");
  }

  async function act(kind: "approve" | "reject") {
    if (!b || !openId) return;
    setBusy(true);
    try {
      const r = kind === "approve" ? await b.approve(openId) : await b.reject(openId);
      setNotice(actionNotice(r));
      setCards(await b.inbox());
    } catch (e) {
      setNotice(String(e));
    } finally {
      setBusy(false);
    }
  }

  const openCard = cards.find((c) => c.id === openId) ?? null;

  return (
    <main className="review">
      <h2>{ui.review.title}</h2>
      {cards.length === 0 && <p data-testid="inbox-empty">{ui.review.empty}</p>}
      <ul data-testid="review-cards">
        {cards.map((c) => (
          <li key={c.id} data-testid={`card-${c.id}`}>
            <button data-testid={`open-${c.id}`} onClick={() => open(c)}>
              <strong>{c.summary}</strong>
              <span>
                {c.author_display} · {c.origin_label}
                {c.needs_reconfirmation && (
                  <em data-testid={`reconfirm-${c.id}`}> · {ui.review.reconfirmBadge}</em>
                )}
              </span>
            </button>
          </li>
        ))}
      </ul>
      {openCard && (
        <section data-testid="detail">
          <header>
            <h3>{openCard.summary}</h3>
            <span data-testid="detail-author">
              {openCard.author_display} · {openCard.origin_label}
            </span>
          </header>
          <div className="side-by-side" data-testid="diff-view">
            <div>
              <h4>{ui.review.before}</h4>
              <pre>
                {chunks.map((c, i) =>
                  c.changed_left ? (
                    <mark key={i}>{c.left || "∅"}</mark>
                  ) : (
                    <span key={i}>{c.left}</span>
                  )
                )}
              </pre>
            </div>
            <div>
              <h4>{ui.review.after}</h4>
              <pre>
                {chunks.map((c, i) =>
                  c.changed_right ? (
                    <mark key={i}>{c.right || "∅"}</mark>
                  ) : (
                    <span key={i}>{c.right}</span>
                  )
                )}
              </pre>
            </div>
          </div>
          <button data-testid="approve-btn" disabled={busy} onClick={() => act("approve")}>
            {ui.review.approve}
          </button>
          <button data-testid="reject-btn" disabled={busy} onClick={() => act("reject")}>
            {ui.review.reject}
          </button>
          {notice && (
            <p role="status" data-testid="action-notice">
              {notice}
            </p>
          )}
        </section>
      )}
    </main>
  );
}
