// 워크스페이스 화면 — M2(E2E-4 직접 부): 트리·CodeMirror 편집·미리보기·
// 검색→위치 점프·저장→변경 세트 제출(F6 — 직접 쓰기 없음).
import { useEffect, useMemo, useRef, useState } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { markdown } from "@codemirror/lang-markdown";
import { defaultKeymap } from "@codemirror/commands";
import { renderMarkdown, byteOffsetToCharIndex } from "../workspace/preview";
import { VirtualList } from "../workspace/VirtualList";
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
  saveDocument(
    path: string,
    content: string,
    image?: { name: string; b64: string }
  ): Promise<{ pr_number: number; summary: string }>;
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
  const [pendingImage, setPendingImage] = useState<{ name: string; b64: string } | null>(null);
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

  async function onPickImage(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    const b64 = await fileToB64(file);
    // 편집 중 문서에 자산 링크 삽입(상대 경로 — F30)
    const link = `![${file.name.replace(/\.[^.]+$/, "")}](../자산/${file.name})`;
    setPendingImage({ name: file.name, b64 });
    setContent((c) => c + (c.endsWith("\n") || c === "" ? "" : "\n") + link);
  }

  function fileToB64(file: File): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => {
        const r = String(reader.result);
        resolve(r.slice(r.indexOf(",") + 1)); // data:...;base64, 제거
      };
      reader.onerror = () => reject(new Error("이미지를 읽지 못했어요"));
      reader.readAsDataURL(file);
    });
  }

  async function save() {
    if (!b || !openPath) return;
    setSaveState("saving");
    try {
      const r = await b.saveDocument(openPath, content, pendingImage ?? undefined);
      setSaveState("done");
      setOriginal(content);
      setPendingImage(null);
      setSaveMessage(`${ui.workspace.saved}: ${r.summary}`);
      // 검토함 갱신 — 목록 재조회
      setEntries(await b.listDir(""));
    } catch (e) {
      setSaveState("error");
      setSaveMessage(String(e));
    }
  }

  const dirty = content !== original;
  const isHtml = openPath?.toLowerCase().endsWith(".html") || openPath?.toLowerCase().endsWith(".htm");
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
        <div data-testid="doc-tree">
          <VirtualList
            items={entries.filter((e) => e.kind !== "Asset")}
            height={440}
            renderItem={(e) => (
              <button
                data-testid={`tree-${e.name}`}
                style={{ width: "100%", textAlign: "left" }}
                onClick={() => (e.kind === "Document" || e.kind === "ReadOnly") && openDoc(e.path)}
              >
                {e.kind === "Folder" ? "📁" : "📄"} {e.name}
              </button>
            )}
          />
        </div>
      </aside>
      <section>
        {openPath === null ? (
          <p data-testid="empty-state">{ui.workspace.pickPrompt}</p>
        ) : (
          <>
            <header>
              <span data-testid="open-path">{openPath}</span>
              {!isHtml && (
                <button
                  data-testid="preview-toggle"
                  onClick={() => setPreview((p) => !p)}
                >
                  {preview ? ui.workspace.editMode : ui.workspace.previewMode}
                </button>
              )}
              {!isHtml && (
              <label className="image-pick">
                {ui.workspace.addImage}
                <input
                  data-testid="image-input"
                  type="file"
                  accept="image/*"
                  onChange={onPickImage}
                  hidden
                />
              </label>
              )}
              {!isHtml && (
                <button
                  data-testid="save-btn"
                  disabled={(!dirty && !pendingImage) || saveState === "saving"}
                  onClick={save}
                >
                  {saveState === "saving" ? ui.workspace.saving : ui.workspace.save}
                </button>
              )}
            </header>
            {isHtml ? (
              <iframe
                data-testid="readonly-html"
                title={openPath}
                sandbox=""
                srcDoc={content}
                style={{ width: "100%", height: "60vh", border: "1px solid #ccd" }}
              />
            ) : preview ? (
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
