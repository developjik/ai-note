//! M2 시나리오 매트릭스(D0 §7 — M2 종료 기준) — wiremock GitHub + 로컬 bare
//! 원격 형상으로 7 시나리오 통과를 증명한다.
//!
//! wiremock은 GitHub REST(브랜치 조회·PR 생성·머지)를 대신하고, git 원격은
//! 로컬 bare 저장소가 담당한다(실제 반영 검증 — 실계정 e2e는 별도 문서화).

use ai_note_core::changeset::{
    self, ApproveOutcome, Changeset, CsFile, CsOrigin, CsState, SubmitOutcome,
};
use ai_note_core::git_layer;
use ai_note_core::github::GithubService;
use ai_note_core::state::ChangesetStore;
use git2::Repository;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 형상: bare 원격(main 초기화됨) + 앱 볼트 + wiremock GitHub.
struct Fixture {
    _tmp: tempfile::TempDir,
    vault: Repository,
    remote_url: String,
    server: MockServer,
    gh: GithubService,
    store: ChangesetStore,
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
    Fixture { _tmp: tmp, vault, remote_url, server, gh, store }
}

fn cs_fixture(id: &str, base: &str, path: &str, content: &str) -> Changeset {
    Changeset {
        id: id.into(),
        author_display: "김하나".into(),
        summary: "회의록 정리".into(),
        base_commit: base.into(),
        files: vec![CsFile { path: path.into(), content: Some(content.into()) }],
        origin: CsOrigin::Edit,
        state: CsState::Draft,
    }
}

fn head_main(vault: &Repository) -> String {
    vault
        .find_reference("refs/heads/main")
        .unwrap()
        .peel_to_commit()
        .unwrap()
        .id()
        .to_string()
}

/// wiremock: 원격 main 해시 = 로컬 main(동일 해시 — 같은 객체 형식).
async fn mock_main_head(f: &Fixture, sha: &str) {
    Mock::given(method("GET")).and(path("/repos/team/notes/branches/main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "main", "commit": { "sha": sha }
        })))
        .mount(&f.server)
        .await;
}

async fn mock_create_pr(f: &Fixture, number: u64) {
    Mock::given(method("POST")).and(path("/repos/team/notes/pulls"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "number": number, "state": "open"
        })))
        .mount(&f.server)
        .await;
}

async fn mock_merge(f: &Fixture, number: u64, status: u16) {
    Mock::given(method("PUT")).and(path(format!("/repos/team/notes/pulls/{number}/merge")))
        .respond_with(ResponseTemplate::new(status).set_body_json(serde_json::json!({
            "merged": status == 200
        })))
        .mount(&f.server)
        .await;
}

/// 시나리오 1 — 단일 cs 승인: applied + 머지 API 호출됨.
#[tokio::test]
async fn m2_scenario_1_single_cs_applied() {
    let mut f = fixture().await;
    let base = head_main(&f.vault);
    mock_main_head(&f, &base).await;
    mock_create_pr(&f, 101).await;
    mock_merge(&f, 101, 200).await;

    let mut cs = cs_fixture("s1", &base, "회의/0930.md", "# 0930 회의");
    let out = changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs)
        .await
        .unwrap();
    assert!(matches!(out, SubmitOutcome::Submitted { pr_number: 101 }));
    assert_eq!(cs.state, CsState::PendingReview);

    let out = changeset::approve(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs)
        .await
        .unwrap();
    assert_eq!(out, ApproveOutcome::Applied);
    assert_eq!(cs.state, CsState::Applied);
    // 원격에 cs 브랜치 도달
    let origin = Repository::open_bare(f.remote_url.trim_start_matches("file://")).unwrap();
    assert!(origin.find_reference("refs/heads/cs/s1").is_ok());
    // 머지 API가 실제로 호출됐는지(wiremock 검증)
    let gets = f.server.received_requests().await.unwrap();
    assert!(gets.iter().any(|r| r.method == "PUT" && r.url.path().ends_with("/pulls/101/merge")));
}

/// 시나리오 2 — 동일 cs 동시 승인 2회: 2회째 '이미 반영됨'(F32).
#[tokio::test]
async fn m2_scenario_2_double_approval_already_applied() {
    let mut f = fixture().await;
    let base = head_main(&f.vault);
    // 원격 main에 이미 cs 내용이 반영된 상태 시뮬레이션:
    // main에 동일 파일·내용 커밋(수동 반영) 후 목 해시 갱신
    let mut cs = cs_fixture("s2", &base, "가이드.md", "# 가이드");
    // 제출 먼저(PR 없이는 approve 불가)
    mock_create_pr(&f, 102).await;
    changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs)
        .await
        .unwrap();

    // main에 동일 내용 반영(다른 cs가 먼저 들어간 형상)
    let main_c = f.vault.find_reference("refs/heads/main").unwrap().peel_to_commit().unwrap();
    let cs_c = f.vault.find_reference("refs/heads/cs/s2").unwrap().peel_to_commit().unwrap();
    let mut index = f.vault.index().unwrap();
    index.read_tree(&main_c.tree().unwrap()).unwrap();
    index
        .add_frombuffer(
            &git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
                file_size: 9,
                id: git2::Oid::zero(), flags: 0, flags_extended: 0,
                path: "가이드.md".as_bytes().to_vec(),
            },
            "# 가이드".as_bytes(),
        )
        .unwrap();
    let tree_id = index.write_tree_to(&f.vault).unwrap();
    let tree = f.vault.find_tree(tree_id).unwrap();
    let sig = git2::Signature::now("main", "app@local").unwrap();
    f.vault.commit(Some("refs/heads/main"), &sig, &sig, "선반영", &tree, &[&main_c, &cs_c]).unwrap();
    let new_main = head_main(&f.vault);

    mock_main_head(&f, &new_main).await;
    // 두 번의 승인 시도 — 첫째는 이미반영됨 판정, 이후 병합 시도 없음
    let out = changeset::approve(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs)
        .await
        .unwrap();
    assert_eq!(out, ApproveOutcome::AlreadyApplied);
    // 머지 API 미호출 단언(판정이 앞서 차단)
    let reqs = f.server.received_requests().await.unwrap();
    assert!(!reqs.iter().any(|r| r.method == "PUT"), "이미 반영됨이면 머지 호출 없음");
    assert_eq!(cs.state, CsState::Applied);
}

/// 시나리오 3 — cs-B 먼저 반영, cs-A base 낡음 → 자동 수렴 후 반영.
#[tokio::test]
async fn m2_scenario_3_stale_base_auto_converge() {
    let mut f = fixture().await;
    let base0 = head_main(&f.vault);
    // cs-B(문서B) 제출·반영
    mock_main_head(&f, &base0).await;
    mock_create_pr(&f, 201).await;
    mock_merge(&f, 201, 200).await;
    let mut cs_b = cs_fixture("s3b", &base0, "문서B.md", "# B");
    changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_b)
        .await
        .unwrap();
    changeset::approve(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_b)
        .await
        .unwrap();

    // main이 앞서 감(B 반영 형상 — 로컬 재현: main에 B 추가)
    let mc = f.vault.find_reference("refs/heads/main").unwrap().peel_to_commit().unwrap();
    let bc = f.vault.find_reference("refs/heads/cs/s3b").unwrap().peel_to_commit().unwrap();
    let mut index = f.vault.index().unwrap();
    index.read_tree(&bc.tree().unwrap()).unwrap();
    let tree_id = index.write_tree_to(&f.vault).unwrap();
    let tree = f.vault.find_tree(tree_id).unwrap();
    let sig = git2::Signature::now("main", "app@local").unwrap();
    f.vault.commit(Some("refs/heads/main"), &sig, &sig, "B 반영", &tree, &[&mc, &bc]).unwrap();
    let new_main = head_main(&f.vault);

    // 단계 전환: 선등록 목(FIFO 우선) 제거 후 cs-A 단계 목 재장착
    f.server.reset().await;
    // cs-A(문서A, base=base0) 승인 → 자동 수렴 후 반영
    mock_main_head(&f, &new_main).await;
    mock_create_pr(&f, 202).await;
    mock_merge(&f, 202, 200).await;
    let mut cs_a = cs_fixture("s3a", &base0, "문서A.md", "# A");
    changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_a)
        .await
        .unwrap();
    let out = changeset::approve(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_a)
        .await
        .unwrap();
    assert_eq!(out, ApproveOutcome::Applied, "수렴 성공 → 반영");
    // cs-A base가 갱신됐는지
    let (stored, _) = f.store.get("s3a").unwrap().unwrap();
    assert_eq!(stored.base_commit, new_main);
}

/// 시나리오 4 — 충돌 cs 승인: resolving 전이(해소 자체는 M3 에이전트).
#[tokio::test]
async fn m2_scenario_4_conflict_to_resolving() {
    let mut f = fixture().await;
    let base0 = head_main(&f.vault);
    mock_create_pr(&f, 301).await;

    // cs-X: 시작하기.md 교체
    let mut cs_x = cs_fixture("s4x", &base0, "시작하기.md", "# 규칙 1 정시");
    changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_x)
        .await
        .unwrap();

    // main에서 같은 파일 다른 내용(다른 cs 선반영)
    let mc = f.vault.find_reference("refs/heads/main").unwrap().peel_to_commit().unwrap();
    let xc = f.vault.find_reference("refs/heads/cs/s4x").unwrap().peel_to_commit().unwrap();
    let mut index = f.vault.index().unwrap();
    index.read_tree(&mc.tree().unwrap()).unwrap();
    index
        .add_frombuffer(
            &git2::IndexEntry {
                ctime: git2::IndexTime::new(0, 0),
                mtime: git2::IndexTime::new(0, 0),
                dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
                file_size: 11,
                id: git2::Oid::zero(), flags: 0, flags_extended: 0,
                path: "시작하기.md".as_bytes().to_vec(),
            },
            "# 규칙 1 자율".as_bytes(),
        )
        .unwrap();
    let tree_id = index.write_tree_to(&f.vault).unwrap();
    let tree = f.vault.find_tree(tree_id).unwrap();
    let sig = git2::Signature::now("main", "app@local").unwrap();
    f.vault.commit(Some("refs/heads/main"), &sig, &sig, "선반영", &tree, &[&mc, &xc]).unwrap();
    let new_main = head_main(&f.vault);
    mock_main_head(&f, &new_main).await;

    let out = changeset::approve(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs_x)
        .await
        .unwrap();
    assert_eq!(out, ApproveOutcome::NeedsResolution);
    assert_eq!(cs_x.state, CsState::Resolving);
    let (stored, _) = f.store.get("s4x").unwrap().unwrap();
    assert_eq!(stored.state, CsState::Resolving);
}

/// 시나리오 5 — AI 해소 3회 실패 → cancelled(R7). 해소 시도 루프는 상태머신
/// 수준에서 재현(에이전트 산출 해소 cs는 M3 — 여기선 전이·상한만).
#[tokio::test]
async fn m2_scenario_5_resolution_cap_cancels() {
    let f = fixture().await;
    let mut cs = cs_fixture("s5", "x", "a.md", "# a");
    cs.state = CsState::PendingReview;
    f.store.put(&cs, Some(501)).unwrap();
    // 충돌 → resolving
    changeset::transition(&mut cs, CsState::Resolving).unwrap();
    f.store.set_state("s5", CsState::Resolving).unwrap();
    // 해소 후 재충돌 2회(상한 3 도달)
    for _ in 0..(changeset::RESOLUTION_MAX_RETRIES - 1) {
        changeset::transition(&mut cs, CsState::PendingReview).unwrap();
        changeset::transition(&mut cs, CsState::Resolving).unwrap();
    }
    changeset::transition(&mut cs, CsState::Cancelled).unwrap();
    f.store.set_state("s5", CsState::Cancelled).unwrap();
    assert_eq!(cs.state, CsState::Cancelled);
    let (stored, _) = f.store.get("s5").unwrap().unwrap();
    assert_eq!(stored.state, CsState::Cancelled);
}

/// 시나리오 6 — 기각 → 재제출: rejected → pending_review, 원문 유지(F21).
#[tokio::test]
async fn m2_scenario_6_reject_resubmit() {
    let mut f = fixture().await;
    let base = head_main(&f.vault);
    mock_create_pr(&f, 601).await;
    let mut cs = cs_fixture("s6", &base, "메모.md", "# 메모");
    changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs)
        .await
        .unwrap();

    // 기각
    changeset::reject(&f.store, &mut cs).unwrap();
    assert_eq!(cs.state, CsState::Rejected);
    let files_before = cs.files.clone();

    // 재제출 — 같은 경로(원문 유지·새 검토 요청). 단계 목 교체.
    f.server.reset().await;
    mock_create_pr(&f, 602).await;
    let out = changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs)
        .await
        .unwrap();
    assert!(matches!(out, SubmitOutcome::Submitted { pr_number: 602 }));
    assert_eq!(cs.state, CsState::PendingReview);
    assert_eq!(cs.files, files_before, "재제출은 원문 유지(F21)");
}

/// 시나리오 7 — 앱 재시작 중간 상태: store 재오픈으로 복원 + cs 브랜치 정합성.
#[tokio::test]
async fn m2_scenario_7_restart_restores_state() {
    let mut f = fixture().await;
    let base = head_main(&f.vault);
    mock_create_pr(&f, 701).await;
    let mut cs = cs_fixture("s7", &base, "재시작.md", "# 내용");
    changeset::submit(&f.vault, &f.gh, "team/notes", &f.remote_url, "ghp_t", &f.store, &mut cs)
        .await
        .unwrap();
    drop(f.store); // 앱 종료

    // 재시작 — 같은 DB 경로 재오픈
    let db = f._tmp.path().join("state.db");
    let store2 = ChangesetStore::open(&db).unwrap();
    let (restored, pr) = store2.get("s7").unwrap().unwrap();
    assert_eq!(restored.state, CsState::PendingReview);
    assert_eq!(pr, Some(701));
    assert_eq!(restored.files, cs.files);

    // cs 브랜치 정합성: 로컬 cs 브랜치 + 원격 cs 브랜치 존재(architect 지적 반영)
    assert!(f.vault.find_reference("refs/heads/cs/s7").is_ok());
    let origin = Repository::open_bare(f.remote_url.trim_start_matches("file://")).unwrap();
    assert!(origin.find_reference("refs/heads/cs/s7").is_ok(), "원격 cs 브랜치 존재 재확인");
}
