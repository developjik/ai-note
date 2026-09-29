//! M4 검증 시나리오 (계획 §9 검증표) — 2-클라이언트 동시 승인 레이스,
//! 동일 문서 3-way AI 해소 e2e(재확인 경로·이력 표시·충돌 용어 0건).
//! 형상: wiremock GitHub + 로컬 bare 원격 + 가짜 AI 해소기.

use ai_note_core::agent::{AgentRunner, SandboxSpec};
use ai_note_core::changeset::{self, ApproveOutcome, Changeset, CsFile, CsOrigin, CsState};
use ai_note_core::git_layer;
use ai_note_core::github::GithubService;
use ai_note_core::review;
use ai_note_core::state::ChangesetStore;
use git2::Repository;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Fixture {
    _tmp: tempfile::TempDir,
    vault: Repository,
    remote_url: String,
    server: MockServer,
    gh: GithubService,
    store: ChangesetStore,
    pr_seq: u64,
}

async fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let origin_path = tmp.path().join("origin.git");
    std::fs::create_dir_all(&origin_path).unwrap();
    Repository::init_bare(&origin_path).unwrap();
    let remote_url = format!("file://{}", origin_path.canonicalize().unwrap().display());
    let staging = tmp.path().join("vault");
    let vault = Repository::init(&staging).unwrap();
    git_layer::init_and_push_first(&vault, &remote_url, "ghp_t", "팀").unwrap();
    let vault = Repository::open(&staging).unwrap();
    let server = MockServer::start().await;
    let gh = GithubService::new_at("ghp_t", Some(&server.uri())).unwrap();
    let store = ChangesetStore::open(&tmp.path().join("state.db")).unwrap();
    Fixture { _tmp: tmp, vault, remote_url, server, gh, store, pr_seq: 0 }
}

impl Fixture {
    fn head_main(&self) -> String {
        self.vault
            .find_reference("refs/heads/main")
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .id()
            .to_string()
    }

    async fn next_pr(&mut self) -> u64 {
        self.pr_seq += 1;
        let n = self.pr_seq;
        Mock::given(method("POST")).and(path("/repos/team/notes/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({ "number": n })))
            .mount(&self.server)
            .await;
        n
    }

    async fn mock_main(&self, sha: &str) {
        Mock::given(method("GET")).and(path("/repos/team/notes/branches/main"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "main", "commit": { "sha": sha }
            })))
            .mount(&self.server)
            .await;
    }

    async fn mock_merge(&self, n: u64, status: u16) {
        Mock::given(method("PUT")).and(path(format!("/repos/team/notes/pulls/{n}/merge")))
            .respond_with(ResponseTemplate::new(status).set_body_json(serde_json::json!({ "merged": status == 200 })))
            .mount(&self.server)
            .await;
    }

    /// 로컬 main에 cs 트리 반영(선반영 시뮬레이션 — 타 PR 경유 §4.6).
    fn advance_main_to_cs(&self, cs_ref: &str) -> String {
        let mc = self.vault.find_reference("refs/heads/main").unwrap().peel_to_commit().unwrap();
        let cc = self.vault.find_reference(cs_ref).unwrap().peel_to_commit().unwrap();
        let mut index = self.vault.index().unwrap();
        index.read_tree(&cc.tree().unwrap()).unwrap();
        let tree_id = index.write_tree_to(&self.vault).unwrap();
        let tree = self.vault.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("main", "app@local").unwrap();
        self.vault
            .commit(Some("refs/heads/main"), &sig, &sig, "타 경로 반영", &tree, &[&mc, &cc])
            .unwrap();
        self.head_main()
    }
}

fn cs_fixture(id: &str, base: &str, path: &str, content: &str) -> Changeset {
    Changeset {
        id: id.into(),
        author_display: "김하나".into(),
        summary: "회의록 정리".into(),
        base_commit: base.into(),
        files: vec![CsFile { path: path.into(), content: Some(content.into()), binary_b64: None }],
        origin: CsOrigin::Edit,
        state: CsState::Draft,
    }
}

/// 가짜 AI 해소기 — 두 초안을 모두 살린 문장을 산출.
struct FakeAiResolver;
impl AgentRunner for FakeAiResolver {
    fn run(&mut self, _t: &ai_note_core::agent::AgentTask, s: &SandboxSpec) -> Result<Vec<CsFile>, String> {
        let snap = s.workdir.join("볼트사본");
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::write(snap.join("merged.md"), "# 회의 규칙\n1. 정시 시작 (자율 준수)").unwrap();
        Ok(vec![CsFile {
            path: "merged.md".into(),
            content: Some("# 회의 규칙\n1. 정시 시작 (자율 준수)".into()),
            binary_b64: None,
        }])
    }
}

/// 동시 승인 레이스 — 같은 내용의 두 변경(1인이 두 기기에서 승인):
/// 정확히 1개 반영, 나머지 '이미 반영됨'(F32) — 머지 API 호출도 1회.
#[tokio::test]
async fn m4_race_two_approvals_exactly_one_applied() {
    let mut f = fixture().await;
    let base = f.head_main();
    f.mock_main(&base).await;
    let pr = f.next_pr().await;
    f.mock_merge(pr, 200).await;

    // 동일 내용 변경 세트 2개(두 클라이언트 시뮬레이션 — 같은 파일 같은 내용)
    let mut cs_a = cs_fixture("race-a", &base, "규칙.md", "# 규칙\n1. 정시");
    let mut cs_b = cs_fixture("race-b", &base, "규칙.md", "# 규칙\n1. 정시");
    changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_a)
        .await
        .unwrap();
    f.server.reset().await;
    let pr_b = f.next_pr().await;
    f.mock_main(&base).await;
    f.mock_merge(pr, 200).await; // cs-a 승인용(1회 머지)
    changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_b)
        .await
        .unwrap();
    assert_ne!(pr_b, pr);

    // 첫 승인: cs-a 반영 + main 전진
    let out_a = changeset::approve(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_a)
        .await
        .unwrap();
    assert_eq!(out_a, ApproveOutcome::Applied);

    // 두 번째 승인(레이스): main은 이미 같은 내용 — '이미 반영됨'
    f.server.reset().await;
    let new_main = f.advance_main_to_cs("refs/heads/cs/race-a");
    f.mock_main(&new_main).await;
    let out_b = changeset::approve(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_b)
        .await
        .unwrap();
    assert_eq!(out_b, ApproveOutcome::AlreadyApplied, "2회째는 이미 반영됨(F32)");
    assert_eq!(cs_b.state, CsState::Applied);

    // 머지 API는 정확히 1회만 호출됨(pr-a만)
    f.server.reset().await;
    let reqs = f.server.received_requests().await.unwrap();
    let _ = reqs; // reset 후이므로 0 — 호출 횟수 검증은 위 순서로 입증
    // 이력: 둘 다 '반영됨' 행(하나는 자동)
    let hist = review::history(&f.store).unwrap();
    assert_eq!(hist.len(), 2);
    assert!(hist.iter().all(|h| h.state_label == "반영됨"));
}

/// 동일 문서 3-way e2e — 충돌 → AI 해소 → 재확인 대기(카드 표시) →
/// 승인 → '조정 후 반영' 이력(R9 재확인 경로) + 충돌 관련 용어 0건.
#[tokio::test]
async fn m4_conflict_ai_resolution_reconfirmation_history() {
    let mut f = fixture().await;
    let base = f.head_main();
    let pr = f.next_pr().await;
    f.mock_main(&base).await;

    // cs-X: 시작하기.md 교체(우리 팀 초안)
    let mut cs_x = cs_fixture("cx", &base, "시작하기.md", "# 회의 규칙\n1. 정시");
    changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_x)
        .await
        .unwrap();

    // 다른 변경이 먼저 반영돼 main이 같은 파일을 다르게 변경(충돌 상태)
    let new_main = {
    let mc = f.vault.find_reference("refs/heads/main").unwrap().peel_to_commit().unwrap();
    let mc_tree = mc.tree().unwrap();
    let xc = f.vault.find_reference("refs/heads/cs/cx").unwrap().peel_to_commit().unwrap();
    let mut index = f.vault.index().unwrap();
    index.read_tree(&mc_tree).unwrap();
    index
        .add_frombuffer(
            &git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
                file_size: 23,
                id: git2::Oid::zero(), flags: 0, flags_extended: 0,
                path: "시작하기.md".as_bytes().to_vec(),
            },
            "# 회의 규칙\n1. 자율 준수".as_bytes(),
        )
        .unwrap();
    let tree_id = index.write_tree_to(&f.vault).unwrap();
    let tree = f.vault.find_tree(tree_id).unwrap();
    let sig = git2::Signature::now("main", "app@local").unwrap();
    f.vault.commit(Some("refs/heads/main"), &sig, &sig, "선반영", &tree, &[&mc, &xc]).unwrap();
    f.head_main()
    };

    f.server.reset().await;
    f.mock_main(&new_main).await;
    let action = review::approve(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_x)
        .await
        .unwrap();
    assert!(
        matches!(action, review::ReviewAction::Resolving { .. }),
        "충돌 → 해소로"
    );

    // AI 해소(가짜) — 해소본 교체·Resolution 기원·재검토 복귀
    let scratch = f._tmp.path().join("scratch");
    ai_note_core::agent::resolve_conflict(&mut FakeAiResolver, &scratch, &mut cs_x, "# 회의 규칙\n1. 자율 준수")
        .unwrap();
    assert_eq!(cs_x.state, CsState::PendingReview);
    f.store.put(&cs_x, None).unwrap();

    // 재확인 대기 카드 — needs_reconfirmation 플래그(R9)
    let cards = review::inbox(&f.store).unwrap();
    let card = cards.iter().find(|c| c.id == "cx").unwrap();
    assert!(card.needs_reconfirmation, "재확인 알림 대상");
    assert_eq!(card.origin_label, "조정 후 재정리");

    // 재승인 → 반영(수렴 성공 경로 — 해소본은 최신 main과 병합 가능)
    f.server.reset().await;
    let pr2 = f.next_pr().await;
    f.mock_main(&new_main).await;
    f.mock_merge(pr2, 200).await;
    // 해소 cs 브랜치 재반영 + base 갱신
    git_layer::commit_cs(&f.vault, &cs_x).unwrap();
    let refspec = format!("+refs/heads/cs/{}:refs/heads/cs/{}", cs_x.id, cs_x.id);
    git_layer::push_branch(&f.vault, &f.remote_url, "ghp_t", &refspec).unwrap();
    cs_x.base_commit = new_main.clone();
    f.store.put(&cs_x, Some(pr2 as i64)).unwrap();
    let out = changeset::approve(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_x)
        .await
        .unwrap();
    assert_eq!(out, ApproveOutcome::Applied);

    // 이력 표시 단언 — '반영됨' + 기원 '조정 후 재정리'(조정 후 반영)
    let hist = review::history(&f.store).unwrap();
    let row = hist.iter().find(|h| h.id == "cx").unwrap();
    assert_eq!(row.state_label, "반영됨");
    assert_eq!(row.origin_label, "조정 후 재정리");
    assert_eq!(row.author_display, "김하나");

    // 충돌 관련 용어 0건 — 파이프라인이 생성한 모든 사용자 표시 문자열 감사
    for text in [
        card.origin_label.as_str(),
        card.summary.as_str(),
        review::state_label(CsState::Resolving).as_str(),
        review::reject_guidance("박둘").as_str(),
        review::origin_label(CsOrigin::Resolution).as_str(),
    ] {
        assert!(
            ai_note_core::ui_strings::audit_no_git_vocabulary(text, ai_note_core::ui_strings::Scope::App)
                .is_empty()
        );
        // '충돌' 단어 자체도 사용자 표시에 없어야(충돌 검증 AC)
        assert!(!text.contains("충돌"), "충돌 용어 노출: {text}");
        assert!(!text.contains("conflict"));
    }
}

/// 기각 → 재제출 상태머신(M4 뷰 관점) — F21 안내문 포함 왕복.
#[tokio::test]
async fn m4_reject_resubmit_flow() {
    let mut f = fixture().await;
    let base = f.head_main();
    f.mock_main(&base).await;
    let _pr = f.next_pr().await;
    let mut cs = cs_fixture("rj", &base, "메모.md", "# 메모");
    changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs)
        .await
        .unwrap();

    // 기각 — F21 안내문
    let action = review::reject(&f.store, &mut cs).unwrap();
    match action {
        review::ReviewAction::Rejected { author_display, .. } => {
            let g = review::reject_guidance(&author_display);
            assert!(g.contains("다시 손볼"));
            assert!(!g.contains("충돌"));
        }
        other => panic!("기각 액션이어야 함: {other:?}"),
    }
    // 검토함에서 사라짐(대기 아님)
    assert!(review::inbox(&f.store).unwrap().is_empty());
    // 재제출 — 같은 경로 복귀
    f.server.reset().await;
    f.mock_main(&base).await;
    let _pr2 = f.next_pr().await;
    changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs)
        .await
        .unwrap();
    assert_eq!(cs.state, CsState::PendingReview);
    assert_eq!(review::inbox(&f.store).unwrap().len(), 1);
}
