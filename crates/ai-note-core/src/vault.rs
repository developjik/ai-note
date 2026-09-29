//! md 볼트 파일 계층 (M2 — 계획 §4 `vault`).
//!
//! 순수 표준 md만 취급(F11)·이미지는 자산 폴더 보관(F30)·폴더 트리 열람·
//! html은 읽기 전용 미리보기(F12). 모든 쓰기는 변경 세트(changeset)를
//! 통해서만 — 이 모듈은 읽기 전용 뷰다(F6).

use git2::Repository;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TreeEntry {
    /// 표시 이름(파일명/폴더명).
    pub name: String,
    /// 볼트 내 전체 경로(폴더는 접미 '/' 포함).
    pub path: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub enum EntryKind {
    Folder,
    Document,
    /// 자산(이미지 등 — 자산 폴더 하위, F30).
    Asset,
    /// html 등 읽기 전용 미리보기 대상(F12).
    ReadOnly,
}

/// main 커밋(원격 추적 전용 — 로컬 main은 읽기 뷰) 트리에서 볼트 트리 열람.
/// depth=1(해당 폴더 직계 자식만) — 화면 가상화를 위한 증분 열람.
pub fn list_dir(vault: &Repository, dir: &str) -> Result<Vec<TreeEntry>, String> {
    let main = vault
        .find_reference("refs/heads/main")
        .map_err(|e| format!("팀 노트가 아직 준비되지 않았어요: {e}"))?
        .peel_to_commit()
        .map_err(|e| e.to_string())?;
    let tree = main.tree().map_err(|e| e.to_string())?;

    let target = dir.trim_matches('/');
    let sub = if target.is_empty() {
        tree.clone()
    } else {
        let entry = tree
            .get_path(std::path::Path::new(target))
            .map_err(|_| format!("그런 폴더가 없어요: {dir}"))?;
        entry
            .to_object(vault)
            .map_err(|e| e.to_string())?
            .peel_to_tree()
            .map_err(|_| format!("폴더가 아니에요: {dir}"))?
    };

    let mut entries: Vec<TreeEntry> = Vec::new();
    for item in sub.iter() {
        let name = item.name().unwrap_or_default().to_string();
        if name.is_empty() || name == ".gitkeep" {
            continue;
        }
        match item.kind() {
            Some(git2::ObjectType::Tree) => entries.push(TreeEntry {
                name: name.clone(),
                path: format!("{target}/{name}/"),
                kind: EntryKind::Folder,
            }),
            Some(git2::ObjectType::Blob) => {
                let full = if target.is_empty() {
                    name.clone()
                } else {
                    format!("{target}/{name}")
                };
                let kind = classify(&full);
                entries.push(TreeEntry { name: name.clone(), path: full, kind });
            }
            _ => {}
        }
    }
    // 폴더 먼저, 그다음 문서 — 가나다 정렬
    entries.sort_by(|a, b| {
        let ka = matches!(a.kind, EntryKind::Folder);
        let kb = matches!(b.kind, EntryKind::Folder);
        kb.cmp(&ka).then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}

fn classify(path: &str) -> EntryKind {
    if path.starts_with("자산/") {
        EntryKind::Asset
    } else if path.to_lowercase().ends_with(".html") || path.to_lowercase().ends_with(".htm") {
        EntryKind::ReadOnly
    } else {
        EntryKind::Document
    }
}

/// 문서 읽기 — md는 원문, 이미지·html은 원문(화면이 읽기 전용 렌더).
pub fn read_file(vault: &Repository, path: &str) -> Result<Vec<u8>, String> {
    let main = vault
        .find_reference("refs/heads/main")
        .map_err(|e| format!("팀 노트가 아직 준비되지 않았어요: {e}"))?
        .peel_to_commit()
        .map_err(|e| e.to_string())?;
    let tree = main.tree().map_err(|e| e.to_string())?;
    let entry = tree
        .get_path(std::path::Path::new(path))
        .map_err(|_| format!("그런 문서가 없어요: {path}"))?;
    let blob = entry
        .to_object(vault)
        .map_err(|e| e.to_string())?
        .peel_to_blob()
        .map_err(|e| e.to_string())?;
    Ok(blob.content().to_vec())
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
                    std::fs::write(tmp.path().join(path), content).unwrap();
                }
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
    fn m2_tree_lists_folders_first_sorted() {
        let (_tmp, repo) = vault_with_files(&[
            ("회의/b.md", "b"),
            ("회의/a.md", "a"),
            ("시작하기.md", "x"),
            ("메모/c.md", "c"),
        ]);
        let root = list_dir(&repo, "").unwrap();
        let names: Vec<(&String, EntryKind)> = root.iter().map(|e| (&e.name, e.kind)).collect();
        assert_eq!(names[0].0, "메모");
        assert_eq!(names[0].1, EntryKind::Folder);
        assert_eq!(names[1].0, "회의");
        assert_eq!(names[2].0, "시작하기.md");
        assert_eq!(names[2].1, EntryKind::Document);

        let 회의 = list_dir(&repo, "회의").unwrap();
        assert_eq!(회의.len(), 2);
        assert_eq!(회의[0].name, "a.md");
    }

    #[test]
    fn m2_asset_and_html_classification() {
        let (_tmp, repo) = vault_with_files(&[
            ("자산/사진.png", "bin"),
            ("보고서.html", "<h1>읽기전용</h1>"),
            ("일반.md", "md"),
        ]);
        let root = list_dir(&repo, "").unwrap();
        let find = |n: &str| root.iter().find(|e| e.name == n).unwrap().kind;
        assert_eq!(find("보고서.html"), EntryKind::ReadOnly);
        assert_eq!(find("일반.md"), EntryKind::Document);
        // 자산 폴더 안의 이미지 — 자산 분류(F30)
        let assets = list_dir(&repo, "자산").unwrap();
        assert_eq!(assets[0].kind, EntryKind::Asset);
        assert_eq!(assets[0].name, "사진.png");
    }

    #[test]
    fn m2_read_document_and_missing_error() {
        let (_tmp, repo) = vault_with_files(&[("회의/a.md", "# 제목")]);
        let content = read_file(&repo, "회의/a.md").unwrap();
        assert_eq!(String::from_utf8_lossy(&content), "# 제목");
        let err = read_file(&repo, "없음.md").unwrap_err();
        assert!(err.contains("그런 문서가 없어요"));
    }

    #[test]
    fn m2_gitkeep_hidden_from_tree() {
        let (_tmp, repo) = vault_with_files(&[("자산/.gitkeep", ""), ("노트.md", "n")]);
        let root = list_dir(&repo, "").unwrap();
        assert!(!root.iter().any(|e| e.name == ".gitkeep"));
        // .gitkeep만 남은 자산 폴더 자체는 표시되되 안은 비어 보임
        let assets = list_dir(&repo, "자산").unwrap();
        assert!(assets.is_empty(), "키퍼 파일은 숨겨짐: {assets:?}");
    }
}
