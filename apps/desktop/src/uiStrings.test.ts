import { describe, expect, it } from "vitest";
import { ui, auditNoGitVocabulary } from "./uiStrings";

describe("ui 문자열 git 어휘 감사 (A1)", () => {
  it("모든 사용자 표시 문자열에 git 어휘가 없다", () => {
    const flatten = (o: unknown): string[] =>
      typeof o === "string"
        ? [o]
        : Object.values(o as Record<string, unknown>).flatMap(flatten);
    for (const text of flatten(ui)) {
      expect(auditNoGitVocabulary(text), `"${text}"에 금지 어휘`).toEqual([]);
    }
  });

  it("감사기가 침입을 잡는다", () => {
    expect(auditNoGitVocabulary("변경 사항을 푸시했습니다").length).toBeGreaterThan(0);
    expect(auditNoGitVocabulary("create branch").length).toBeGreaterThan(0);
  });
});
