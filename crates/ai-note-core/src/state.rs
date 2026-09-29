//! 로컬 앱 상태 (M1: 연결 상태 JSON / M2: SQLite 변경 세트 저장소).
//!
//! SQLite: 변경 세트 상태·PR 메타데이터+작성자 표시명(E5)·GitHub API
//! 캐시(ETag)·검색 인덱스 위치. 온라인 전제(F19)로 오프라인 대기열 없음.
//! 계정 토큰은 절대 여기 두지 않는다(keyring 전용, F14).
//!
//! 시나리오 7(앱 재시작 중간 상태): 모든 쓰기는 즉시 커밋되고 재시작 시
//! `load_all`로 상태 복원.

use crate::changeset::{Changeset, CsFile, CsOrigin, CsState};
use rusqlite::Connection;
use std::path::Path;

// ── M1: 연결 상태 JSON ─────────────────────────────────────────────

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct AppState {
    /// 연결된 팀 볼트 저장소 이름(없으면 미연결).
    pub connected_repo: Option<String>,
    /// 저장소 소유 계정(승인 로그인 — PR API 경로용).
    #[serde(default)]
    pub owner: Option<String>,
    /// 관리자(첫 사용자) 여부 — F16 혼합 모델 표시용.
    pub is_admin: bool,
}

pub fn state_path(app_data: &Path) -> std::path::PathBuf {
    app_data.join("app-state.json")
}

pub fn save(app_data: &Path, state: &AppState) -> Result<(), String> {
    std::fs::create_dir_all(app_data).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(state_path(app_data), json).map_err(|e| e.to_string())
}

pub fn load(app_data: &Path) -> AppState {
    std::fs::read_to_string(state_path(app_data))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

// ── M2: 변경 세트 SQLite 저장소 ────────────────────────────────────

/// 변경 세트 저장소 — cs 상태 영속(D0 §4: 전이는 SQLite에 영속).
pub struct ChangesetStore {
    conn: Connection,
}

impl ChangesetStore {
    /// 저장소 열기/생성(스키마 자동 마이그레이션 v1).
    pub fn open(db_path: &Path) -> Result<Self, String> {
        if let Some(dir) = db_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS changesets (
                id TEXT PRIMARY KEY,
                author_display TEXT NOT NULL,
                summary TEXT NOT NULL,
                base_commit TEXT NOT NULL,
                origin TEXT NOT NULL,
                state TEXT NOT NULL,
                pr_number INTEGER,
                files_json TEXT NOT NULL,
                resolution_retries INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_changesets_state ON changesets(state);",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    /// 메모리 저장소(테스트·시나리오).
    pub fn open_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS changesets (
                id TEXT PRIMARY KEY,
                author_display TEXT NOT NULL,
                summary TEXT NOT NULL,
                base_commit TEXT NOT NULL,
                origin TEXT NOT NULL,
                state TEXT NOT NULL,
                pr_number INTEGER,
                files_json TEXT NOT NULL,
                resolution_retries INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    /// 변경 세트 저장(신규·갱신 모두 — 전이 검증은 changeset::transition이
    /// 선행한다. 여기는 영속 계층이다).
    pub fn put(&self, cs: &Changeset, pr_number: Option<i64>) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO changesets (id, author_display, summary, base_commit, origin, state, pr_number, files_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                    state=excluded.state, summary=excluded.summary,
                    base_commit=excluded.base_commit, origin=excluded.origin,
                    pr_number=COALESCE(excluded.pr_number, pr_number),
                    files_json=excluded.files_json, updated_at=datetime('now')",
                rusqlite::params![
                    cs.id,
                    cs.author_display,
                    cs.summary,
                    cs.base_commit,
                    serde_json::to_string(&cs.origin).unwrap(),
                    serde_json::to_string(&cs.state).unwrap(),
                    pr_number,
                    serde_json::to_string(&cs.files).unwrap(),
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_state(&self, id: &str, state: CsState) -> Result<(), String> {
        let n = self
            .conn
            .execute(
                "UPDATE changesets SET state=?2, updated_at=datetime('now') WHERE id=?1",
                rusqlite::params![id, serde_json::to_string(&state).unwrap()],
            )
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err(format!("없는 변경 세트예요: {id}"));
        }
        Ok(())
    }

    pub fn set_pr(&self, id: &str, pr_number: i64) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE changesets SET pr_number=?2, updated_at=datetime('now') WHERE id=?1",
                rusqlite::params![id, pr_number],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn row_to_cs(row: &rusqlite::Row<'_>) -> rusqlite::Result<(Changeset, Option<i64>)> {
        let origin: String = row.get(4)?;
        let state: String = row.get(5)?;
        let files: String = row.get(7)?;
        Ok((
            Changeset {
                id: row.get(0)?,
                author_display: row.get(1)?,
                summary: row.get(2)?,
                base_commit: row.get(3)?,
                origin: serde_json::from_str(&origin).unwrap_or(CsOrigin::Edit),
                state: serde_json::from_str(&state).unwrap_or(CsState::Draft),
                files: serde_json::from_str::<Vec<CsFile>>(&files).unwrap_or_default(),
            },
            row.get(6)?,
        ))
    }

    pub fn get(&self, id: &str) -> Result<Option<(Changeset, Option<i64>)>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, author_display, summary, base_commit, origin, state, pr_number, files_json FROM changesets WHERE id=?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query(rusqlite::params![id]).map_err(|e| e.to_string())?;
        match rows.next().map_err(|e| e.to_string())? {
            Some(row) => Ok(Some(Self::row_to_cs(row).map_err(|e| e.to_string())?)),
            None => Ok(None),
        }
    }

    /// 상태별 목록(검토함 = pending_review, 이력 = applied/rejected/cancelled).
    pub fn list_by_state(&self, state: CsState) -> Result<Vec<(Changeset, Option<i64>)>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, author_display, summary, base_commit, origin, state, pr_number, files_json FROM changesets WHERE state=?1 ORDER BY updated_at DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![serde_json::to_string(&state).unwrap()], Self::row_to_cs)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// 해소 재시도 횟수(R7).
    pub fn get_resolution_retries(&self, id: &str) -> Result<u32, String> {
        self.conn
            .query_row(
                "SELECT resolution_retries FROM changesets WHERE id=?1",
                rusqlite::params![id],
                |r| r.get::<_, i64>(0),
            )
            .map(|v| v.max(0) as u32)
            .map_err(|e| e.to_string())
    }

    pub fn set_resolution_retries(&self, id: &str, n: u32) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE changesets SET resolution_retries=?2, updated_at=datetime('now') WHERE id=?1",
                rusqlite::params![id, n as i64],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 전체 개수(진단).
    pub fn count(&self) -> Result<i64, String> {
        self.conn
            .query_row("SELECT COUNT(*) FROM changesets", [], |r| r.get(0))
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m1_state_roundtrip_and_default() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(tmp.path()), AppState::default());
        let st = AppState {
            connected_repo: Some("team-notes".into()),
            owner: Some("team-ainote".into()),
            is_admin: true,
        };
        save(tmp.path(), &st).unwrap();
        assert_eq!(load(tmp.path()), st);
        std::fs::write(state_path(tmp.path()), "{깨진").unwrap();
        assert_eq!(load(tmp.path()), AppState::default());
    }

    fn sample(id: &str, state: CsState) -> Changeset {
        Changeset {
            id: id.into(),
            author_display: "김하나".into(),
            summary: "회의록 정리".into(),
            base_commit: "abc".into(),
            files: vec![CsFile { path: "회의/a.md".into(), content: Some("# A".into()), binary_b64: None }],
            origin: CsOrigin::Edit,
            state,
        }
    }

    /// 시나리오 7 — 앱 재시작 중간 상태: SQLite 재오픈으로 상태 복원.
    #[test]
    fn m2_store_survives_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("state.db");

        let s1 = ChangesetStore::open(&db).unwrap();
        s1.put(&sample("cs-1", CsState::PendingReview), None).unwrap();
        s1.set_pr("cs-1", 42).unwrap();
        s1.put(&sample("cs-2", CsState::Applied), Some(7)).unwrap();
        drop(s1); // 앱 종료

        // 재시작 — 복원
        let s2 = ChangesetStore::open(&db).unwrap();
        assert_eq!(s2.count().unwrap(), 2);
        let (cs1, pr1) = s2.get("cs-1").unwrap().unwrap();
        assert_eq!(cs1.state, CsState::PendingReview);
        assert_eq!(cs1.files.len(), 1);
        assert_eq!(pr1, Some(42));
        let pending = s2.list_by_state(CsState::PendingReview).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0.id, "cs-1");
        let applied = s2.list_by_state(CsState::Applied).unwrap();
        assert_eq!(applied[0].1, Some(7));
    }

    #[test]
    fn m2_store_set_state_and_missing_errors_friendly() {
        let s = ChangesetStore::open_memory().unwrap();
        s.put(&sample("cs-9", CsState::Draft), None).unwrap();
        s.set_state("cs-9", CsState::PendingReview).unwrap();
        assert_eq!(s.get("cs-9").unwrap().unwrap().0.state, CsState::PendingReview);
        let err = s.set_state("없는놈", CsState::Applied).unwrap_err();
        assert!(err.contains("없는 변경 세트"));
    }

    /// put 갱신 경로 — 상태·파일 갱신 시 PR 번호는 기존값 유지(COALESCE).
    #[test]
    fn m2_store_update_preserves_pr_number() {
        let s = ChangesetStore::open_memory().unwrap();
        s.put(&sample("cs-a", CsState::PendingReview), Some(11)).unwrap();
        s.put(&sample("cs-a", CsState::Resolving), None).unwrap();
        let (cs, pr) = s.get("cs-a").unwrap().unwrap();
        assert_eq!(cs.state, CsState::Resolving);
        assert_eq!(pr, Some(11), "재저장이 PR 번호를 지우면 안 됨");
    }
}
