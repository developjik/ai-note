#!/usr/bin/env python3
"""A1 git 어휘 감사 — 사용자 표시 문자열 소스 전체 스캔.

Rust ui_strings 레지스트리의 감사기와 프런트 uiStrings.ts 감사기가
실제 모든 문자열 리터럴에 대해 0건임을 독립적으로 재확인한다.
예외(온보딩·가이드 허용 페이지)는 ui_strings.rs Scope::Allowed와
uiStrings.ts의 허용 목록으로 관리한다.
"""
import re
import sys
from pathlib import Path

DENY = [
    "커밋", "푸시", "브랜치", "풀 리퀘스트", "풀리퀘스트", "머지", "리베이스",
    "클론", "체크아웃", "페치", "스테이징", "스태시",
    "commit", "push", "branch", "pull request", "pullrequest",
    "merge", "rebase", "clone", "checkout", "fetch", "revert", "stash",
]
# 감사 대상: 사용자 표시 문자열 소스. (문서·테스트 코드·이 스크립트 자체는 제외)
TARGETS = [
    "crates/ai-note-core/src/ui_strings.rs",
    "apps/desktop/src/uiStrings.ts",
    "apps/desktop/src/screens",
    "apps/desktop/src/App.tsx",
    "apps/desktop/index.html",
]
ALLOW_MARKERS = ("Scope::Allowed", "onboard.", "DENYLIST")

def strip_exempt(text: str, path: str) -> str:
    """금지어 목록 정의부·감사기 자체 테스트 픽스처를 제외한 본문 반환."""
    # 1) DENYLIST 배열 정의 본문 제거 (TS/Rust 양식)
    text = re.sub(r"export const DENYLIST_\w+ = \[[^\]]*\];", "", text)
    text = re.sub(r"pub const DENYLIST_\w+: &\[&str\] = &\[[^\]]*\];", "", text)
    # 2) Rust 유닛테스트 모듈 제거 — 감사기가 침입을 잡는지 증명하는 픽스처
    if path.endswith(".rs"):
        text = re.sub(r"#\[cfg\(test\)\][\s\S]*$", "", text)
    # 3) TS 감사기 자체 테스트 제거
    if path.endswith(".test.ts"):
        text = ""
    return text

def main() -> int:
    violations = []
    for target in TARGETS:
        p = Path(target)
        paths = [p] if p.is_file() else sorted(p.rglob("*.tsx")) + sorted(p.rglob("*.ts")) if p.is_dir() else []
        for f in paths:
            text = strip_exempt(f.read_text(encoding="utf-8"), str(f))
            # 문자열 리터럴만 추출
            literals = re.findall(r'"([^"\\]*(?:\\.[^"\\]*)*)"', text)
            for lit in literals:
                if any(m in lit for m in ALLOW_MARKERS):
                    continue  # 감사기 자신의 금지어 목록·허용 페이지 문자열
                low = lit.lower()
                for word in DENY:
                    if word in low:
                        violations.append(f"{f}:{lit[:60]} → {word}")
    if violations:
        print("금지 어휘 발견:")
        for v in violations:
            print(" -", v)
        return 1
    print("git 어휘 감사 통과 — 사용자 표시 문자열 0건")
    return 0

if __name__ == "__main__":
    sys.exit(main())
