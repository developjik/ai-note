// 미리보기 렌더러 + 점프 오프셋 변환 단위 테스트 — M2(F11/F12, E2E-4 점프).
import { describe, expect, it } from "vitest";
import { renderMarkdown, parseInline, byteOffsetToCharIndex } from "./preview";

describe("마크다운 미리보기 (M2)", () => {
  it("제목·문단·굵게·코드 인라인 해석", () => {
    const nodes = renderMarkdown("# 제목\n\n본문 **강조**와 `코드`\n");
    expect(nodes[0].kind).toBe("h1");
    const p = nodes[1];
    expect(p.kind).toBe("p");
    const kinds = (p.children ?? []).map((c) => c.t);
    expect(kinds).toContain("bold");
    expect(kinds).toContain("code");
  });

  it("목록 묶음", () => {
    const nodes = renderMarkdown("- 하나\n- 둘\n");
    expect(nodes[0].kind).toBe("ul");
    expect((nodes[0].children ?? []).length).toBeGreaterThanOrEqual(2);
  });

  it("코드 블록·인용·수평선", () => {
    const nodes = renderMarkdown("```\nconst x = 1;\n```\n> 인용\n\n---\n");
    expect(nodes.some((n) => n.kind === "code")).toBe(true);
    expect(nodes.some((n) => n.kind === "quote")).toBe(true);
    expect(nodes.some((n) => n.kind === "hr")).toBe(true);
  });

  it("이미지는 자산 경로로(링크와 구분)", () => {
    const inl = parseInline("사진 ![다이어그램](자산/flow.png) 참조");
    const img = inl.find((i) => i.t === "img") as Extract<Inline, { t: "img" }> | undefined;
    expect(img?.src).toBe("자산/flow.png");
    const link = parseInline("[문서](회의/0930.md)");
    expect(link.some((i) => i.t === "link")).toBe(true);
  });

  it("검색 점프: 바이트 오프셋 → 문자 인덱스(한국어 UTF-8)", () => {
    const doc = "회의록 시작\n오늘 의제: 출시 일정 조정";
    // '출시'의 바이트 오프셋 계산(렌더러 밖 계산 재현)
    const byteAt = new TextEncoder().encode(doc.slice(0, doc.indexOf("출시"))).length;
    const charAt = byteOffsetToCharIndex(doc, byteAt);
    expect(doc.slice(charAt, charAt + 2)).toBe("출시");
  });

  it("빈 문서·경계 오프셋 안전", () => {
    expect(renderMarkdown("")).toEqual([]);
    expect(byteOffsetToCharIndex("abc", 99)).toBe(3);
    expect(byteOffsetToCharIndex("한글", 0)).toBe(0);
  });
});

type Inline = ReturnType<typeof parseInline>[number];
