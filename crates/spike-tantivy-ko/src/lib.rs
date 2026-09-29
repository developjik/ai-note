//! S3 스파이크 — 한국어 바이그램 토크나이저 전문 검색 (계획 §4 `search`, P5).
//!
//! 증명 항목(M0-S3):
//! 1. 바이그램 토크나이저가 형태소 분석 없이 한국어 부분일치 검색을 잡는다
//!    (무사전 — lindera-ko는 v2 재검토, 교체 지점 추상화 유지)
//! 2. 파일명+내용 혼합 질의(F24)가 결과를 올바른 문서로 보낸다
//! 3. 수천 문서 규모에서 인덱스가 디스크에 합리적으로 유지된다(F33 규모 상한)
//!
//! 실행: `cargo test -p spike-tantivy-ko -- --nocapture`
//! 결과 요약: docs/spikes/S3-tantivy-ko.md

use anyhow::Result;
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, TextFieldIndexing, TextOptions, Value};
use tantivy::{doc, Index, IndexReader, IndexWriter, ReloadPolicy};

/// 한국어 바이그램 토크나이저 — 2-gram + 1-gram(홀수 길이 대비) 생성.
///
/// 교체 지점 추상화(P5): 이 타입 뒤로 Tantivy 커스텀 토크나이저를 숨기고
/// v2에서 lindera-ko 등으로 교체할 때 이 구현만 갈아끼운다.
#[derive(Clone, Copy, Debug, Default)]
pub struct KoreanBigramTokenizer;

impl KoreanBigramTokenizer {
    /// 토큰화: 소문자 정규화 + 알파벳/숫자/한글/가나다 유니코드 블록 유지,
    /// 공백·구두점은 구분자. 각 워드에서 2-gram(길이 1이면 그대로).
    pub fn tokens(text: &str) -> Vec<String> {
        let lower: String = text.to_lowercase();
        let mut out = Vec::new();
        for word in lower.split(|c: char| !c.is_alphanumeric()) {
            let chars: Vec<char> = word.chars().collect();
            if chars.is_empty() {
                continue;
            }
            if chars.len() == 1 {
                out.push(chars.iter().collect());
                continue;
            }
            for i in 0..chars.len().saturating_sub(1) {
                out.push(chars[i..i + 2].iter().collect());
            }
            // 마지막 홀수 조각 보존(3글자 word의 끝 글자 짝 보강)
            if chars.len() % 2 == 1 {
                let last: String = chars[chars.len() - 1..].iter().collect();
                out.push(last);
            }
        }
        out
    }
}

/// 검색 문서 — 파일명+내용(F24).
pub struct SearchDoc {
    pub path: String,
    pub title: String,
    pub body: String,
}

/// 인덱스 스키마: path(STORED+INDEXED), title/body(바이그램 TEXT, body stored).
fn schema() -> Schema {
    let mut builder = Schema::builder();
    let gram = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer("ko_bigram")
                .set_index_option(tantivy::schema::IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    builder.add_text_field("path", tantivy::schema::TEXT | tantivy::schema::STORED);
    builder.add_text_field("title", gram.clone());
    builder.add_text_field("body", gram);
    builder.build()
}

pub struct SearchIndex {
    pub index: Index,
    pub reader: IndexReader,
    path_field: tantivy::schema::Field,
    title_field: tantivy::schema::Field,
    body_field: tantivy::schema::Field,
}

impl SearchIndex {
    /// 커스텀 토크나이저 등록은 tantivy Tokenizer 트레이트로 감싸서 수행한다.
    pub fn create(dir: &Path) -> Result<Self> {
        let schema = schema();
        let index = Index::create_in_dir(dir, schema.clone())?;
        Self::open_inner(index)
    }

    pub fn open(dir: &Path) -> Result<Self> {
        let index = Index::open_in_dir(dir)?;
        Self::open_inner(index)
    }

    fn open_inner(index: Index) -> Result<Self> {
        register_korean_bigram(&index);
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        let schema = index.schema();
        Ok(Self {
            index,
            reader,
            path_field: schema.get_field("path")?,
            title_field: schema.get_field("title")?,
            body_field: schema.get_field("body")?,
        })
    }

    /// 문서 추가(증분 갱신) — 커밋 후 수동 리로드.
    pub fn add_documents(&self, writer: &mut IndexWriter, docs: &[SearchDoc]) -> Result<()> {
        for d in docs {
            writer.add_document(doc!(
                self.path_field => d.path.clone(),
                self.title_field => d.title.clone(),
                self.body_field => d.body.clone(),
            ))?;
        }
        Ok(())
    }

    /// 질의: 파일명+제목+내용 통합(F24). 상위 k개의 path 반환.
    pub fn search(&self, query_str: &str, k: usize) -> Result<Vec<String>> {
        let parser = QueryParser::for_index(
            &self.index,
            vec![self.title_field, self.body_field, self.path_field],
        );
        let query = parser.parse_query(query_str)?;
        let searcher = self.reader.searcher();
        let hits = searcher.search(&query, &TopDocs::with_limit(k))?;
        let mut out = Vec::new();
        for (_score, addr) in hits {
            let retrieved = searcher.doc::<tantivy::TantivyDocument>(addr)?;
            let path = retrieved
                .get_first(self.path_field)
                .map(|v| v.as_str().unwrap_or_default().to_string())
                .unwrap_or_default();
            out.push(path);
        }
        Ok(out)
    }
}

/// Tantivy 등록 파이프라인 — 내장 NgramTokenizer(2,2) + LowerCaser.
///
/// S3 발견: tantivy 0.22의 `NgramTokenizer`가 단어 단위 겹침 n-gram을
/// 제공하므로 커스텀 어댑터가 불필요하다. 교체 지점(P5)은 이 함수 하나로,
/// v2에서 lindera-ko로 갈아끼울 때 여기만 바꾼다.
pub fn register_korean_bigram(index: &Index) {
    let analyzer = tantivy::tokenizer::TextAnalyzer::builder(
        tantivy::tokenizer::NgramTokenizer::new(2, 2, false).unwrap(),
    )
    .filter(tantivy::tokenizer::LowerCaser)
    .build();
    index.tokenizers().register("ko_bigram", analyzer);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_docs() -> Vec<SearchDoc> {
        vec![
            SearchDoc {
                path: "회의록/2026-09-29.md".into(),
                title: "주간 회의록".into(),
                body: "김하나가 마케팅 예산 승인을 요청했다. 다음 주 스프린트 계획 포함.".into(),
            },
            SearchDoc {
                path: "개인/독서노트.md".into(),
                title: "독서 노트 — 습관".into(),
                body: "아주 작은 습관이 쌓인다. 실행 방법과 기록 양식.".into(),
            },
            SearchDoc {
                path: "팀/온보딩-가이드.md".into(),
                title: "새 팀원 가이드".into(),
                body: "첫 날 체크리스트. 계정 연결과 문서 작성 규칙.".into(),
            },
        ]
    }

    fn build(dir: &Path) -> SearchIndex {
        let si = SearchIndex::create(dir).unwrap();
        let mut writer = si.index.writer(16 * 1024 * 1024).unwrap();
        si.add_documents(&mut writer, &sample_docs()).unwrap();
        writer.commit().unwrap();
        si.reader.reload().unwrap();
        si
    }

    #[test]
    fn s3_1_korean_partial_match_via_bigram() {
        let tmp = tempfile::tempdir().unwrap();
        let si = build(tmp.path());
        // '예산 승인' — 형태소 분석 없이 바이그램으로 문서 적중
        let hits = si.search("예산", 3).unwrap();
        assert!(hits.iter().any(|p| p.contains("회의록")), "hits={hits:?}");
        let hits = si.search("습관", 3).unwrap();
        assert!(hits.iter().any(|p| p.contains("독서노트")), "hits={hits:?}");
    }

    #[test]
    fn s3_2_title_body_filename_mix() {
        let tmp = tempfile::tempdir().unwrap();
        let si = build(tmp.path());
        // 제목어로 검색
        let hits = si.search("회의록", 3).unwrap();
        assert!(hits.iter().any(|p| p.contains("2026-09-29")));
        // 내용어로 검색
        let hits = si.search("체크리스트", 3).unwrap();
        assert!(hits.iter().any(|p| p.contains("온보딩")));
    }

    #[test]
    fn s3_3_incremental_add_is_searchable() {
        let tmp = tempfile::tempdir().unwrap();
        let si = build(tmp.path());
        let mut writer = si.index.writer(16 * 1024 * 1024).unwrap();
        si.add_documents(
            &mut writer,
            &[SearchDoc {
                path: "새/추가문서.md".into(),
                title: "나중에 추가".into(),
                body: "증분 색인 테스트용 문서".into(),
            }],
        )
        .unwrap();
        writer.commit().unwrap();
        si.reader.reload().unwrap();
        let hits = si.search("증분", 3).unwrap();
        assert!(hits.iter().any(|p| p.contains("추가문서")));
    }

    #[test]
    fn s3_4_scale_and_size_footprint() {
        let tmp = tempfile::tempdir().unwrap();
        let si = SearchIndex::create(tmp.path()).unwrap();
        let mut writer = si.index.writer(32 * 1024 * 1024).unwrap();
        let t0 = std::time::Instant::now();
        let batch: Vec<SearchDoc> = (0..3_000)
            .map(|i| SearchDoc {
                path: format!("볼트/note{i:04}.md"),
                title: format!("문서 {i} — 프로젝트 기록"),
                body: format!(
                    "문서 {i} 본문. 팀 회의와 스프린트 기록, 예산과 승인 노트 {i}. \
                     한국어 바이그램 색인 규모 측정용 샘플 텍스트입니다."
                ),
            })
            .collect();
        si.add_documents(&mut writer, &batch).unwrap();
        writer.commit().unwrap();
        let build_secs = t0.elapsed().as_secs_f64();
        si.reader.reload().unwrap();

        let t1 = std::time::Instant::now();
        let hits = si.search("스프린트", 5).unwrap();
        let query_ms = t1.elapsed().as_millis();
        assert!(!hits.is_empty());

        let index_bytes: u64 = walk_size(tmp.path());
        let mb = index_bytes as f64 / 1e6;
        // 기록용 출력(docs/spikes/S3 문서에 옮겨 적는다)
        println!("build_secs={build_secs:.1} query_ms={query_ms} index_mb={mb:.1} docs=3000");
        // 완화된 상한: 수천 문서 규모에서 인덱스가 비합리적으로 크지 않음(< 500MB)
        assert!(mb < 500.0, "인덱스 과대: {mb:.1}MB");
        // 질의가 즉각적(스파이크 기준 2초 내 — 병목 최적화는 M2)
        assert!(query_ms < 2000, "질의 지연 {query_ms}ms");
    }

    fn walk_size(dir: &Path) -> u64 {
        let mut total = 0;
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    total += walk_size(&p);
                } else {
                    total += e.metadata().map(|m| m.len()).unwrap_or(0);
                }
            }
        }
        total
    }
}
