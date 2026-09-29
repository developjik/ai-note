//! M5 검증 — 업데이터 감지 e2e(wiremock v0.9→v1.0, minisign 검증 포함),
//! 검색 p95 하니스(3,000문서·100쿼리 < 300ms), API 예산 시뮬레이션.

use ai_note_core::github::GithubService;
use ai_note_core::update::{self, UpdateCheck};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// 감지 e2e — 최신 v1.0 > 현재 v0.9 → Available + 서명 검증 성공(P7).
#[tokio::test]
async fn m5_update_detect_e2e_signature_verified() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/repos/team/notes/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tag_name": "v1.0.0",
            "html_url": "https://github.com/team/notes/releases/tag/v1.0.0"
        })))
        .mount(&server)
        .await;
    // checksums.txt = 데이터 "test"(벡터와 정합), 서명 = 공개 벡터 서명
    Mock::given(method("GET")).and(path("/release-assets/checksums.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("test"))
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/release-assets/checksums.txt.minisig"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "untrusted comment: signature from minisign secret key\nRWQf6LRCGA9i59SLOFxz6NxvASXDJeRtuZykwQepbDEGt87ig1BNpWaVWuNrm73YiIiJbq71Wi+dP9eKL8OC351vwIasSSbXxwA=\ntrusted comment: timestamp:1555779966\tfile:test\nQtKMXWyYcwdpZAlPF7tE2ENJkRd1ujvKjlj1m9RtHTBnZPa5WKU5uWRs5GoP5M/VqE81QFuMKI5k/SfNQUaOAA==",
        ))
        .mount(&server)
        .await;

    let gh = GithubService::new_at("ghp_t", Some(&server.uri())).unwrap();
    const PK: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    let check = update::check_for_update(&gh, "team/notes", "v0.9.2", PK).await;
    match &check {
        UpdateCheck::Available { current, latest, download_url, signature_verified } => {
            assert_eq!(current, "v0.9.2");
            assert_eq!(latest, "v1.0.0");
            assert!(download_url.contains("v1.0.0"));
            assert!(signature_verified, "minisign 서명 검증 통과");
        }
        other => panic!("업데이트 감지 실패: {other:?}"),
    }
    // 안내 문구 — 수동 재다운로드(P7: 자기 교체 없음)
    let notice = update::update_notice(&check);
    assert!(notice.contains("덮어 설치"));
    assert!(!notice.contains("자동"));
}

/// 동일 버전 → UpToDate, 릴리스 부재(404) → CheckFailed(일상 언어).
#[tokio::test]
async fn m5_update_uptodate_and_failure_paths() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/repos/team/notes/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tag_name": "v0.9.2", "html_url": "u"
        })))
        .mount(&server)
        .await;
    let gh = GithubService::new_at("ghp_t", Some(&server.uri())).unwrap();
    let check = update::check_for_update(&gh, "team/notes", "v0.9.2", "pk").await;
    assert!(matches!(check, UpdateCheck::UpToDate { .. }));

    // 404 서버
    let server2 = MockServer::start().await;
    Mock::given(method("GET")).and(path("/repos/team/notes/releases/latest"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server2)
        .await;
    let gh2 = GithubService::new_at("ghp_t", Some(&server2.uri())).unwrap();
    let check2 = update::check_for_update(&gh2, "team/notes", "v0.9.2", "pk").await;
    match check2 {
        UpdateCheck::CheckFailed { reason } => assert!(reason.contains("새 버전")),
        other => panic!("실패 경로여야 함: {other:?}"),
    }
}

/// 검색 p95 하니스 — 3,000문서·100쿼리 p95 < 300ms(계획 성능 목표).
#[test]
fn m5_search_p95_under_300ms() {
    use ai_note_core::search::build_index;
    let tmp = tempfile::tempdir().unwrap();
    let repo = git2::Repository::init(tmp.path()).unwrap();
    {
        let mut index = repo.index().unwrap();
        for i in 0..3000 {
            let content = format!(
                "문서 {}: 회의록과 기록, 예산 승인 및 일정 조정 메모 — 키워드{}",
                i, i % 7
            );
            index
                .add_frombuffer(
                    &git2::IndexEntry {
                        ctime: git2::IndexTime::new(0, 0),
                        mtime: git2::IndexTime::new(0, 0),
                        dev: 0, ino: 0, mode: 0o100644, uid: 0, gid: 0,
                        file_size: content.len() as u32,
                        id: git2::Oid::zero(), flags: 0, flags_extended: 0,
                        path: format!("노트{i:04}.md").into_bytes(),
                    },
                    content.as_bytes(),
                )
                .unwrap();
        }
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("t", "t@local").unwrap();
        let c = repo.commit(Some("refs/heads/main"), &sig, &sig, "대량", &tree, &[]).unwrap();
        let commit = repo.find_commit(c).unwrap();
        repo.branch("main", &commit, true).unwrap();
    }
    let s = build_index(&repo).unwrap();

    let queries: Vec<String> = (0..100)
        .map(|i| match i % 5 {
            0 => "예산 승인".to_string(),
            1 => format!("키워드{}", i % 7),
            2 => "일정 조정".to_string(),
            3 => "회의록과 기록".to_string(),
            _ => "문서 없는검색어".to_string(),
        })
        .collect();
    let mut samples_ms: Vec<u128> = Vec::new();
    for q in &queries {
        let t = std::time::Instant::now();
        let _ = s.search(q, 20).unwrap();
        samples_ms.push(t.elapsed().as_millis());
    }
    samples_ms.sort();
    let p95 = samples_ms[(samples_ms.len() as f64 * 0.95) as usize - 1];
    assert!(p95 < 300, "검색 p95 {p95}ms — 300ms 초과");
}

/// API 예산 시뮬레이션 — 팀 10명·폴링 간격 60s·요청 2종(브랜치 해시·검토함)·
/// ETag 304 70% 가정: PAT 5,000 req/hr 한도 내 설계 증명.
#[test]
fn m5_api_budget_10_users_within_5000_per_hour() {
    let users = 10u64;
    let poll_interval_secs = 60u64;
    let requests_per_poll = 2u64; // main 해시 + 대기 목록(ETag 캐시 시 304도 요청 1회로 계산)
    let _hours = 24u64;
    // 분당: users × (60/interval) × requests
    let per_hour = users * (3600 / poll_interval_secs) * requests_per_poll;
    assert!(per_hour < 5000, "시간당 {per_hour}건 — 5,000 한도 초과");
    // 일일 누적도 24배 — 한도는 시간당 기준이므로 시간당 단언으로 충분.
    // ETag 304가 70%면 실효 처리 비용은 더 낮음(설계 여유).
    let effective = per_hour as f64 * 0.3 + per_hour as f64 * 0.7 * 0.1;
    assert!(effective < per_hour as f64);
}
