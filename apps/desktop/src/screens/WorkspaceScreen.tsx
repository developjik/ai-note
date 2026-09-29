// 워크스페이스 화면 — M2(E2E-4 직접 부): 트리·CodeMirror 편집·미리보기·
// 검색→위치 점프·저장→변경 세트 제출(F6 — 직접 쓰기 없음).
import { useEffect, useMemo, useRef, useState } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { markdown } from "@codemirror/lang-markdown";
import { defaultKeymap } from "@codemirror/commands";
import { renderMarkdown, byteOffsetToCharIndex } from "../workspace/preview";
import { ui } from "../uiStrings";

export interface TreeEntryDto {
  name: string;
  path: string;
  kind: "Folder" | "Document" | "Asset" | "ReadOnly";
}

export interface SearchHitDto {
  path: string;
  snippet: string;
  byte_offset: number;
}

export interface WorkspaceBridge {
  listDir(dir: string): Promise<TreeEntryDto[]>;
  readFile(path: string): Promise<string>;
  searchVault(query: string): Promise<SearchHitDto[]>;
  saveDocument(path: string, content: string): Promise<{ pr_number: number; summary: string }>;
}

declare global {
  interface Window { __aiNoteWorkspace?: WorkspaceBridge }
}

export function WorkspaceScreen({ bridge }: { bridge?: WorkspaceBridge }) {
  const b = bridge ?? window.__aiNoteWorkspace;
  const [entries, setEntries] = useState<TreeEntryDto[]>([]);
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [original, setOriginal] = useState("");
  const [preview, setPreview] = useState(false);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHitDto[]>([]);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "done" | "error">("idle");
  const [saveMessage, setSaveMessage] = useState("");
  const viewRef = useRef<EditorView | null>(null);
  const hostRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!b) return;
    b.listDir("").then(setEntries).catch(() => setEntries([]));
  }, [b]);

  // CodeMirror 초기화(문서 열림/내용 변경 시 재구성 — M2 단순 모델)
  useEffect(() => {
    if (!hostRef.current || preview) return;
    const state = EditorState.create({
      doc: content,
      extensions: [
        markdown(),
        keymap.of(defaultKeymap),
        EditorView.updateListener.of((v) => {
          if (v.docChanged) setContent(v.state.doc.toString());
        }),
      ],
    });
    const view = new EditorView({ state, parent: hostRef.current });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, [openPath, preview]); // content는 리스너 경유 — 재초기화 없음

  async function openDoc(path: string) {
    if (!b) return;
    const text = await b.readFile(path);
    setOpenPath(path);
    setContent(text);
    setOriginal(text);
    setSaveState("idle");
    setSaveMessage("");
  }

  async function jumpTo(hit: SearchHitDto) {
    await openDoc(hit.path);
    // 점프: 바이트 오프셋 → 문자 위치 → 커서 이동·스크롤(E2E-4)
    setTimeout(() => {
      const view = viewRef.current;
      if (!view) return;
      const charAt = byteOffsetToCharIndex(view.state.doc.toString(), hit.byte_offset);
      view.dispatch({
        selection: { anchor: charAt },
        scrollIntoView: true,
      });
      view.focus();
    }, 0);
  }

  async function doSearch() {
    if (!b || query.trim().length === 0) {
      setHits([]);
      return;
    }
    setHits(await b.searchVault(query));
  }

  async function save() {
    if (!b || !openPath) return;
    setSaveState("saving");
    try {
      const r = await b.saveDocument(openPath, content);
      setSaveState("done");
      setOriginal(content);
      setSaveMessage(`${ui.workspace.saved}: ${r.summary}`);
      // 검토함 갱신 — 목록 재조회
      setEntries(await b.listDir(""));
    } catch (e) {
      setSaveState("error");
      setSaveMessage(String(e));
    }
  }

  const dirty = content !== original;
  const previewNodes = useMemo(() => (preview ? renderMarkdown(content) : []), [preview, content]);

  return (
    <main className="workspace">
      <aside>
        <input
          data-testid="search-input"
          placeholder={ui.workspace.searchPlaceholder}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && doSearch()}
        />
        {hits.length > 0 && (
          <ul data-testid="search-hits">
            {hits.map((h, i) => (
              <li key={i}>
                <button data-testid={`hit-${i}`} onClick={() => jumpTo(h)}>
                  <strong>{h.path}</strong>
                  <span>{h.snippet}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
        <ul data-testid="doc-tree">
          {entries
            .filter((e) => e.kind !== "Asset")
            .map((e) => (
              <li key={e.path}>
                <button
                  data-testid={`tree-${e.name}`}
                  onClick={() => e.kind === "Document" && openDoc(e.path)}
                >
                  {e.kind === "Folder" ? "📁" : "📄"} {e.name}
                </button>
              </li>
            ))}
        </ul>
      </aside>
      <section>
        {openPath === null ? (
          <p data-testid="empty-state">{ui.workspace.pickPrompt}</p>
        ) : (
          <>
            <header>
              <span data-testid="open-path">{openPath}</span>
              <button
                data-testid="preview-toggle"
                onClick={() => setPreview((p) => !p)}
              >
                {preview ? ui.workspace.editMode : ui.workspace.previewMode}
              </button>
              <button data-testid="save-btn" disabled={!dirty || saveState === "saving"} onClick={save}>
                {saveState === "saving" ? ui.workspace.saving : ui.workspace.save}
              </button>
            </header>
            {preview ? (
              <div data-testid="preview-pane">
                {previewNodes.map((n, i) => (
                  <PreviewBlock key={i} node={n} />
                ))}
              </div>
            ) : (
              <div data-testid="editor-host" ref={hostRef} />
            )}
            {saveMessage && (
              <p role="status" data-testid="save-message">
                {saveMessage}
              </p>
            )}
          </>
        )}
      </section>
    </main>
  );
}

function PreviewBlock({ node }: { node: ReturnType<typeof renderMarkdown>[number] }) {
  const inlines = (node.children ?? []).map((c, i) => {
    switch (c.t) {
      case "bold":
        return <strong key={i}>{c.v}</strong>;
      case "code":
        return <code key={i}>{c.v}</code>;
      case "link":
        return (
          <a key={i} href={c.href} onClick={(e) => e.preventDefault()}>
            {c.v}
          </a>
        );
      default:
        return <span key={i}>{"v" in c ? c.v : ""}</span>;
    }
  });
  switch (node.kind) {
    case "h1":
      return <h1>{inlines}</h1>;
    case "h2":
      return <h2>{inlines}</h2>;
    case "h3":
      return <h3>{inlines}</h3>;
    case "ul":
      return (
        <ul>
          {(node.children ?? []).map((c, i) => (
            <li key={i}>{"v" in c ? c.v : c.alt}</li>
          ))}
        </ul>
      );
    case "code":
      return <pre><code>{node.text}</code></pre>;
    case "quote":
      return <blockquote>{inlines}</blockquote>;
    case "hr":
      return <hr />;
    default:
      return <p>{inlines}</p>;
  }
}
