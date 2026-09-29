//! 사용자 표시 문자열 단일 소스 + git 어휘 감사 (계획 A1·R8·H).
//!
//! 원칙: git 작업 어휘(한국어·영어)가 사용자에게 노출되는 것을 금지한다.
//! 예외 허용 페이지(온보딩·가이드·설정의 GitHub 서비스명·PAT 발급 안내·
//! 무서명 우회 가이드 문구 인용)는 [`Scope::Allowed`]로 명시적으로만 연다.
//! 감사 대상: 이 레지스트리 + 프런트 번들 + Rust 노출 문자열 전부(CI 잡).

/// 문자열이 노출되는 UI 영역 — 예외 페이지만 `Allowed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// 일반 앱 화면(메인·리뷰·이력) — git 어휘 절대 금지.
    App,
    /// 온보딩·가이드·설정 — GitHub 서비스명·PAT 발급·OS 보안 경고 인용 허용.
    Allowed,
}

/// 금지 어휘 — 한국어. (감사 CI와 동일 소스를 쓴다: R8)
pub const DENYLIST_KO: &[&str] = &[
    "커밋", "커밋하", "푸시", "푸시하", "브랜치", "풀 리퀘스트", "풀리퀘스트",
    "머지", "리베이스", "클론", "체크아웃", "페치", "스테이징", "스태시",
];

/// 금지 어휘 — 영어 (대소문자 무시 매칭).
pub const DENYLIST_EN: &[&str] = &[
    "commit", "push", "branch", "pull request", "pull_request", "pullrequest",
    "merge", "rebase", "clone", "checkout", "fetch", "revert", "stash",
];

/// 사용자 표시 문자열 레지스트리 (한국어 v1 — F31).
///
/// 코드 곳곳에 문자열을 흩뿌리지 말고 이 레지스트리를 경유한다.
/// 프런트엔드는 Tauri bridge로 이 값을 받아 렌더한다.
pub fn s(key: &str, scope: Scope) -> &'static str {
    let (text, string_scope): (&str, Scope) = match key {
        // 메인 워크스페이스
        "workspace.title" => ("내 노트", Scope::App),
        "workspace.new_document" => ("새 문서", Scope::App),
        "workspace.search_placeholder" => ("문서 찾기 — 제목이나 내용으로", Scope::App),
        "workspace.search_no_result" => ("찾는 내용이 없어요", Scope::App),
        "workspace.saving" => ("저장 중…", Scope::App),
        "workspace.saved_pending_review" => ("저장했어요 — 팀 검토를 기다리는 중", Scope::App),
        "workspace.image_added" => ("그림을 문서에 넣었어요", Scope::App),
        // AI 채팅
        "agent.title" => ("AI 도우미", Scope::App),
        "agent.working" => ("작업하는 중…", Scope::App),
        "agent.done" => ("작업이 끝났어요 — 검토 요청 드림", Scope::App),
        "agent.failed" => ("작업에 문제가 생겼어요. 다시 시도해 주세요", Scope::App),
        "agent.doc_created" => ("새 문서를 만들었어요", Scope::App),
        "agent.doc_modified" => ("문서를 고쳤어요", Scope::App),
        "agent.doc_deleted" => ("문서를 지웠어요", Scope::App),
        "agent.conflict_resolved" => ("겹친 수정을 정리했어요", Scope::App),
        // 리뷰
        "review.title" => ("검토함", Scope::App),
        "review.summary_card" => ("무엇이 바뀌었나요", Scope::App),
        "review.approve" => ("반영하기", Scope::App),
        "review.reject" => ("도로 돌리기", Scope::App),
        "review.reject_hint" => ("작성자가 다시 손볼 수 있어요", Scope::App),
        "review.already_applied" => ("이미 반영된 수정이에요", Scope::App),
        "review.before" => ("바뀌기 전", Scope::App),
        "review.after" => ("바뀐 후", Scope::App),
        // 이력
        "history.title" => ("지난 기록", Scope::App),
        "history.status_applied" => ("반영됨", Scope::App),
        "history.status_rejected" => ("도로 돌림", Scope::App),
        "history.adjusted_badge" => ("겹친 수정 정리 후 반영", Scope::App),
        // 초대·설정
        "invite.title" => ("초대장", Scope::App),
        "invite.paste_hint" => ("받은 초대장을 여기에 붙여넣어 주세요", Scope::App),
        "invite.connected" => ("우리 팀 노트와 연결됐어요", Scope::App),
        // 온보딩(허용 페이지)
        "onboard.claude_install_title" => ("AI 도우미 깔기", Scope::Allowed),
        "onboard.claude_install_auto" => ("자동으로 깔아볼게요", Scope::Allowed),
        "onboard.claude_install_manual" => ("직접 깔기 — 아래 순서대로 따라 하세요", Scope::Allowed),
        "onboard.signin" => ("Claude 계정으로 시작", Scope::Allowed),
        "onboard.github_note" => ("노트는 GitHub라는 서비스에 안전하게 보관돼요", Scope::Allowed),
        _ => return "",
    };
    debug_assert_eq!(scope, string_scope, "문자열 scope 불일치: {key}");
    text
}

/// 문자열에 금지 git 어휘가 있는지 검사해 위반 어휘를 반환한다.
///
/// `scope == Scope::Allowed`면 빈 벡터를 반환(예외 페이지).
/// CI 감사 잡과 프런트엔드 번들 점검이 같은 함수를 재사용한다.
pub fn audit_no_git_vocabulary(text: &str, scope: Scope) -> Vec<&'static str> {
    if scope == Scope::Allowed {
        return Vec::new();
    }
    let lower = text.to_lowercase();
    let mut hits = Vec::new();
    for word in DENYLIST_KO.iter().chain(DENYLIST_EN.iter()) {
        let probe = word.to_lowercase();
        if probe.len() >= 2 && lower.contains(&probe) {
            hits.push(*word);
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_no_git_vocabulary_on_app_scope() {
        for key in [
            "workspace.title", "workspace.saved_pending_review", "agent.doc_modified",
            "review.approve", "review.already_applied", "history.status_applied",
            "invite.connected", "agent.conflict_resolved",
        ] {
            let text = s(key, Scope::App);
            assert!(!text.is_empty(), "누락된 키: {key}");
            assert!(
                audit_no_git_vocabulary(text, Scope::App).is_empty(),
                "'{text}'에 금지 어휘 포함 (key={key})"
            );
        }
    }

    #[test]
    fn audit_catches_korean_and_english_terms() {
        assert!(!audit_no_git_vocabulary("변경 사항을 원격에 푸시했습니다", Scope::App).is_empty());
        assert!(!audit_no_git_vocabulary("create a new branch", Scope::App).is_empty());
        assert!(!audit_no_git_vocabulary("PR 머지 완료", Scope::App).is_empty());
    }

    #[test]
    fn allowed_scope_exempts_onboarding_pages() {
        assert!(audit_no_git_vocabulary("GitHub 계정 연결·PAT 발급 안내", Scope::Allowed).is_empty());
    }
}
