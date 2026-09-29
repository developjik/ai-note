//! 전문 검색 (M2 — S3 증명 기반 tantivy 색인, F10).
//!
//! 한국어 빅그램(S3: 내장 NgramTokenizer(2,2)+LowerCaser 채택)로 색인하고
//! 검색 결과에 **본문 오프셋**을 실어 해당 위치 점프(E2E-4)를 지원한다.
//! 볼트는 main 트리가 기준(읽기 전용 뷰 — F6). 재색인은 증분(문서별 해시
//! 비교)이나 M2는 전량 재색인 + 문서 수 기준선(3,000문서) 유지.

use git2::Repository;
use serde::Serialize;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{IndexRecordOption, Schema, TextFieldIndexing, Value};
use tantivy::{doc, Index, TantivyDocument};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchHit {
    pub path: String,
    /// 발췌(쿼리 주변) — 화면 하이라이트용.
    pub snippet: String,
    /// 본문 바이트 오프셋(해당 위치 점프 — E2E-4).
    pub byte_offset: usize,
}

/// 한국어 빅그램 토크나이저 등록(S3 채택 지점 — 교체 필요시 여기만).
fn register_korean_bigram() -> tantivy::tokenizer::TextAnalyzer {
    tantivy::tokenizer::TextAnalyzer::builder(
        tantivy::tokenizer::NgramTokenizer::new(2, 2, false).expect("빅그램 토크나이저"),
    )
    .filter(tantivy::tokenizer::LowerCaser)
    .build()
}

/// main 트리 전체 문서를 색인해 인메모리 검색기를 만든다.
/// 반환 Searcher는 인덱스 소유(스냅숏) — 재색인 주기는 호출자 정책.
pub struct VaultSearch {
    index: Index,
    reader: tantivy::IndexReader,
}

pub fn build_index(vault: &Repository) -> Result<VaultSearch, String> {
    let mut builder = Schema::builder();
    let indexing = TextFieldIndexing::default()
        .set_index_option(IndexRecordOption::WithFreqsAndPositions)
        .set_tokenizer("ko_bigram");
    let text_opts = tantivy::schema::TextOptions::default()
        .set_indexing_options(indexing)
        .set_stored();
    let path_field = builder.add_text_field("path", text_opts.clone());
    let body_field = builder.add_text_field("body", text_opts);
    let schema = builder.build();

    let index = Index::create_in_ram(schema.clone());
    index
        .tokenizers()
        .register("ko_bigram", register_korean_bigram());

    let main = vault
        .find_reference("refs/heads/main")
        .map_err(|e| format!("팀 노트가 아직 준비되지 않았어요: {e}"))?
        .peel_to_commit()
        .map_err(|e| e.to_string())?;
    let tree = main.tree().map_err(|e| e.to_string())?;

    let mut writer = index
        .writer(15_000_000)
        .map_err(|e| format!("색인 준비 실패: {e}"))?;
    let mut indexed = 0usize;
    // 재귀 순회 — 하위 폴더 문서까지 색인(트리 워크)
    fn walk(
        vault: &Repository,
        tree: &git2::Tree<'_>,
        prefix: &str,
        writer: &mut tantivy::IndexWriter,
        path_field: tantivy::schema::Field,
        body_field: tantivy::schema::Field,
        indexed: &mut usize,
    ) -> Result<(), String> {
        for item in tree.iter() {
            let name = item.name().unwrap_or_default().to_string();
            let full = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            match item.kind() {
                Some(git2::ObjectType::Tree) => {
                    let sub = item
                        .to_object(vault)
                        .map_err(|e| e.to_string())?
                        .peel_to_tree()
                        .map_err(|e| e.to_string())?;
                    walk(vault, &sub, &full, writer, path_field, body_field, indexed)?;
                }
                Some(git2::ObjectType::Blob) => {
                    if !full.to_lowercase().ends_with(".md") {
                        continue;
                    }
                    let blob = item
                        .to_object(vault)
                        .map_err(|e| e.to_string())?
                        .peel_to_blob()
                        .map_err(|e| e.to_string())?;
                    let content = String::from_utf8_lossy(blob.content()).to_string();
                    writer
                        .add_document(doc!(
                            path_field => full.clone(),
                            body_field => content,
                        ))
                        .map_err(|e| format!("문서 색인 실패({full}): {e}"))?;
                    *indexed += 1;
                }
                _ => {}
            }
        }
        Ok(())
    }
    walk(vault, &tree, "", &mut writer, path_field, body_field, &mut indexed)?;
    writer.commit().map_err(|e| format!("색인 확정 실패: {e}"))?;
    let reader = index
        .reader()
        .map_err(|e| format!("색인 읽기 준비 실패: {e}"))?;

    let _ = indexed; // 진단용 — 대량 기준선 테스트가 상위에서 측정
    Ok(VaultSearch { index, reader })
}

impl VaultSearch {
    /// 검색 — 빅그램 토큰화된 본문 일치. 최대 limit건(기본 20).
    /// 스니펫은 본문에서 쿼리 첫 토큰 주변 ±40자, byte_offset은 그 지점.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, String> {
        let searcher = self.reader.searcher();
        let path_field = self
            .index
            .schema()
            .get_field("path")
            .map_err(|e| e.to_string())?;
        let body_field = self
            .index
            .schema()
            .get_field("body")
            .map_err(|e| e.to_string())?;

        let parser = QueryParser::for_index(&self.index, vec![body_field]);
        // 빅그램 인덱스: 2자 미만 쿼리는 접두 확장으로 처리(빅그램 특성)
        let qtext = if query.chars().count() < 2 {
            format!("\"{query}\"")
        } else {
            query.to_string()
        };
        let parsed = parser
            .parse_query(&qtext)
            .map_err(|_| format!("찾을 수 없는 검색어예요: {query}"))?;

        let top = searcher
            .search(&parsed, &TopDocs::with_limit(limit))
            .map_err(|e| format!("검색 실패: {e}"))?;

        let mut hits = Vec::new();
        for (_score, addr) in top {
            let retrieved: TantivyDocument = searcher
                .doc::<TantivyDocument>(addr)
                .map_err(|e| e.to_string())?;
            let path = retrieved
                .get_first(path_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let body = retrieved
                .get_first(body_field)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let (snippet, offset) = snippet_around(&body, query);
            hits.push(SearchHit { path, snippet, byte_offset: offset });
        }
        Ok(hits)
    }
}

/// 본문에서 쿼리 첫 등장 위치 주변 발췌 + 바이트 오프셋(점프용).
fn snippet_around(body: &str, query: &str) -> (String, usize) {
    // 대소문자 무시 첫 등장 탐색(바이트 인덱스 유지 — UTF-8 안전 슬라이스)
    let hay = body.to_lowercase();
    let needle = query.to_lowercase();
    match hay.find(&needle) {
        Some(byte_at) => {
            let start = byte_at.saturating_sub(40);
            // UTF-8 경계 보정
            let start = fix_boundary(body, start, true);
            let end = fix_boundary(body, (byte_at + needle.len() + 40).min(body.len()), false);
            let snip = format!(
                "{}[{}]{}",
                &body[start..byte_at],
                &body[byte_at..(byte_at + needle.len()).min(body.len())],
                &body[(byte_at + needle.len()).min(body.len())..end]
            );
            (snip.replace('\n', " "), byte_at)
        }
        None => (body.chars().take(80).collect::<String>().replace('\n', " "), 0),
    }
}

fn fix_boundary(s: &str, mut i: usize, back: bool) -> usize {
    while i > 0 && i < s.len() && !s.is_char_boundary(i) {
        i = if back { i - 1 } else { i + 1 };
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_with_files(files: &[(&str, &str)]) -> (tempfile::TempDir, Repository) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        {
            let mut index = repo.index().unwrap();
            for (path, content) in files {
                if let Some(dir) = std::path::Path::new(path).parent() {
                    std::fs::create_dir_all(tmp.path().join(dir)).unwrap();
                }
                std::fs::write(tmp.path().join(path), content).unwrap();
                index.add_path(std::path::Path::new(path)).unwrap();
            }
            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let sig = git2::Signature::now("t", "t@local").unwrap();
            let c = repo.commit(Some("refs/heads/main"), &sig, &sig, "init", &tree, &[]).unwrap();
            let commit = repo.find_commit(c).unwrap();
            repo.branch("main", &commit, true).unwrap();
        }
        (tmp, repo)
    }

    #[test]
    fn m2_search_korean_bigram_and_jump_offset() {
        let (_tmp, repo) = vault_with_files(&[
            ("회의/0930.md", "# 주간 회의\n오늘 의제: 출시 일정 조정\n"),
            ("메모/개인.md", "장보기 목록: 우유, 커피"),
        ]);
        let s = build_index(&repo).unwrap();
        let hits = s.search("출시 일정", 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "회의/0930.md");
        assert!(hits[0].snippet.contains("출시 일정"), "스니펫: {}", hits[0].snippet);
        // 점프 오프셋 — 본문 슬라이스가 쿼리 시작과 일치해야
        let body = String::from_utf8_lossy(
            &crate::vault::read_file(&repo, "회의/0930.md").unwrap(),
        )
        .to_string();
        assert!(
            body[hits[0].byte_offset..].starts_with("출시 일정"),
            "오프셋 점프 정합"
        );
    }

    #[test]
    fn m2_search_multiple_hits_ranked_and_limit() {
        let files: Vec<(String, String)> = (0..30)
            .map(|i| (format!("노트{i:02}.md"), format!("회의록 {i} — 키워드 언급")))
            .collect();
        let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let (_tmp, repo) = vault_with_files(&refs);
        let s = build_index(&repo).unwrap();
        let all = s.search("키워드", 20).unwrap();
        assert_eq!(all.len(), 20, "기본 상한");
        let few = s.search("키워드", 5).unwrap();
        assert_eq!(few.len(), 5);
    }

    #[test]
    fn m2_search_no_hit_empty_and_short_query() {
        let (_tmp, repo) = vault_with_files(&[("a.md", "내용 없음")]);
        let s = build_index(&repo).unwrap();
        assert!(s.search("없는단어", 20).unwrap().is_empty());
        // 1글자 쿼리도 죽지 않고 결과 반환(빅그램 특성상 문구 검색)
        let hits = s.search("내", 20);
        assert!(hits.is_ok());
    }

    /// 계획 검증: 3,000문서 인덱스 기준선(S3 재확인 — 색인·검색 상한).
    #[test]
    fn m2_search_3000_docs_baseline() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        {
            let mut index = repo.index().unwrap();
            // 대량 문서는 파일 쓰기 없이 인덱스 직접 구성(속도)
            for i in 0..3000 {
                let content = format!("문서 {i} 번째 본문입니다. 회의와 기록이 쌓여요.");
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
        let t0 = std::time::Instant::now();
        let s = build_index(&repo).unwrap();
        let index_ms = t0.elapsed().as_millis();

        let t1 = std::time::Instant::now();
        let hits = s.search("회의와 기록", 20).unwrap();
        let query_ms = t1.elapsed().as_millis();

        assert!(!hits.is_empty());
        // S3 기준선: 색인 < 5s, 검색 < 500ms(넉넉한 상방 — p95 목표는 M5)
        assert!(index_ms < 5000, "색인 {index_ms}ms");
        assert!(query_ms < 500, "검색 {query_ms}ms");
    }
}
