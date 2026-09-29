// 마크다운 미리보기 렌더러 — M2(F11 표준 md, F12 html은 읽기 전용).
// 순수 함수: XSS 방지 위해 태그 생성은 textContent 조립만 사용(innerHTML 금지).
export interface PreviewNode {
  kind: "h1" | "h2" | "h3" | "p" | "li" | "ul" | "code" | "quote" | "hr" | "img";
  text?: string;
  href?: string;
  children?: Inline[];
}

export type Inline =
  | { t: "text"; v: string }
  | { t: "bold"; v: string }
  | { t: "code"; v: string }
  | { t: "link"; v: string; href: string }
  | { t: "img"; alt: string; src: string };

/** 인라인 문법 해석 — **굵게**, `코드`, [링크](url), ![이미지](자산/...). */
export function parseInline(line: string): Inline[] {
  const out: Inline[] = [];
  let rest = line;
  const pattern = /(\*\*([^*]+)\*\*)|(`([^`]+)`)|(!\[([^\]]*)\]\(([^)]+)\))|(\[([^\]]+)\]\(([^)]+)\))/;
  while (rest.length > 0) {
    const m = rest.match(pattern);
    if (!m || m.index === undefined) {
      out.push({ t: "text", v: rest });
      break;
    }
    if (m.index > 0) out.push({ t: "text", v: rest.slice(0, m.index) });
    if (m[2] !== undefined) out.push({ t: "bold", v: m[2] });
    else if (m[4] !== undefined) out.push({ t: "code", v: m[4] });
    else if (m[6] !== undefined) out.push({ t: "img", alt: m[6], src: m[7] });
    else if (m[9] !== undefined) out.push({ t: "link", v: m[9], href: m[10] });
    rest = rest.slice(m.index + m[0].length);
  }
  return out;
}

/** 문서 → 미리보기 노드 트리(제목·목록·인용·수평선·문단). */
export function renderMarkdown(src: string): PreviewNode[] {
  const nodes: PreviewNode[] = [];
  const lines = src.split("\n");
  let para: string[] = [];
  let list: string[] | null = null;
  let codeBuf: string[] | null = null;

  const flushPara = () => {
    if (para.length > 0) {
      nodes.push({ kind: "p", children: parseInline(para.join(" ")) });
      para = [];
    }
  };
  const flushList = () => {
    if (list && list.length > 0) {
      nodes.push({
        kind: "ul",
        children: list.flatMap((item) => parseInline(item)),
      });
    }
    list = null;
  };

  for (const raw of lines) {
    const line = raw.replace(/\s+$/, "");
    if (codeBuf !== null) {
      if (line.trim().startsWith("```")) {
        nodes.push({ kind: "code", text: codeBuf.join("\n") });
        codeBuf = null;
      } else {
        codeBuf.push(raw);
      }
      continue;
    }
    if (line.trim().startsWith("```")) {
      flushPara();
      flushList();
      codeBuf = [];
      continue;
    }
    const heading = line.match(/^(#{1,3})\s+(.*)$/);
    if (heading) {
      flushPara();
      flushList();
      nodes.push({
        kind: `h${heading[1].length}` as "h1" | "h2" | "h3",
        children: parseInline(heading[2]),
      });
      continue;
    }
    if (/^(-|\*|\+)\s+/.test(line)) {
      flushPara();
      list = list ?? [];
      list.push(line.replace(/^(-|\*|\+)\s+/, ""));
      continue;
    }
    if (/^>\s?/.test(line)) {
      flushPara();
      flushList();
      nodes.push({ kind: "quote", children: parseInline(line.replace(/^>\s?/, "")) });
      continue;
    }
    if (/^(---|\*\*\*)\s*$/.test(line)) {
      flushPara();
      flushList();
      nodes.push({ kind: "hr" });
      continue;
    }
    if (line.trim() === "") {
      flushPara();
      flushList();
      continue;
    }
    flushList();
    para.push(line);
  }
  flushPara();
  flushList();
  if (codeBuf) nodes.push({ kind: "code", text: codeBuf.join("\n") });
  return nodes;
}

/** 바이트 오프셋(검색 점프) → 문자 인덱스 변환 — 한국어 UTF-8 안전. */
export function byteOffsetToCharIndex(text: string, byteOffset: number): number {
  const enc = new TextEncoder();
  let bytes = 0;
  for (let i = 0; i < text.length; i++) {
    if (bytes >= byteOffset) return i;
    bytes += enc.encode(text[i]).length;
  }
  return text.length;
}
