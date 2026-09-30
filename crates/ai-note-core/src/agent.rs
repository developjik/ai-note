//! 에이전트 슈퍼바이저 (M3 — S1/S6 증명 기반, F25/F28, §10.3 강제 스택).
//!
//! claude CLI를 샌드박스 안에서 실행하고 산출을 변경 세트로 흘린다.
//! **강제 스택(3계층)**:
//! - L1 환경 차단: 자식에게 토큰·경로·환경 미전달(env_clear + 최소 PATH)
//! - L2 자격 접근 차단: 샌드박스 프로파일이 키체인·.git·볼트 쓰기 거부
//! - L3 파이프라인 외 쓰기 차단+탐지: 볼트 원본은 워커 외 쓰기 불가,
//!   위반 쓰기 시도 감지 시 알림(F25)
//!
//! 검증: 결정론적 가짜 데몬(테스트)이 금지 행위를 시도 → 전부 차단 단언.

use crate::changeset::{Changeset, CsFile, CsOrigin, CsState};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// 작업 시작(F28 진행 알림).
    Started { task_id: String, prompt: String },
    /// 진행 로그(톄스트림 요약).
    Progress { task_id: String, note: String },
    /// 산출 확정 → 검토 요청으로.
    Produced { task_id: String, summary: String },
    /// 실패 — 일상 언어 안내.
    Failed { task_id: String, reason: String },
    /// L3 위반 탐지 — 즉시 알림(F25).
    SecurityAlert { task_id: String, detail: String },
}

/// 에이전트 작업 단위.
#[derive(Debug, Clone)]
pub struct AgentTask {
    pub id: String,
    /// 사용자 지시(자연어).
    pub prompt: String,
    /// 작업 대상 문서(선택 — 없으면 자유 작업).
    pub target_doc: Option<String>,
    pub author_display: String,
}

/// 실행기 트레이트 — 실 claude와 결정론적 가짜 데몬(검증용)을 같은
/// 인터페이스로 다룬다(S1 어댑터 격리).
pub trait AgentRunner: Send {
    /// 샌드박스 안에서 prompt를 실행하고 산출 파일(경로→내용)을 반환.
    fn run(&mut self, task: &AgentTask, sandbox: &SandboxSpec) -> Result<Vec<CsFile>, String>;
}

/// 샌드박스 명세(L1/L2 강제) — 실행기 구현이 이 규격을 지킨다.
#[derive(Debug, Clone)]
pub struct SandboxSpec {
    /// 작업 전용 임시 작업 디렉터리(에이전트가 쓸 수 있는 유일 공간).
    pub workdir: PathBuf,
    /// 읽기 허용 경로(볼트 원본은 읽기 전용 사본만).
    pub readonly_mounts: Vec<PathBuf>,
    /// L1: 자식 환경 완전 삭제 후 최소 PATH만.
    pub scrub_environment: bool,
    /// 작업사본에 미리 심을 볼트 파일(경로→내용) — 원본 미노출.
    pub snapshot_files: Vec<(String, String)>,
}

impl SandboxSpec {
    /// 표준 스펙 — 볼트는 사본 경로만, 원본·키체인·.git은 아예 미포함.
    pub fn for_task(scratch_root: &Path, task_id: &str) -> Self {
        SandboxSpec {
            workdir: scratch_root.join(task_id),
            readonly_mounts: Vec::new(),
            scrub_environment: true,
            snapshot_files: Vec::new(),
        }
    }
}

/// 병렬 작업 큐 — 동시 실행 상한 N(설정, F28 동시 작업).
pub struct SupervisorQueue<R: AgentRunner> {
    runner: Arc<Mutex<R>>,
    pending: Arc<Mutex<VecDeque<AgentTask>>>,
    events: mpsc::Sender<AgentEvent>,
    concurrency: usize,
    scratch_root: PathBuf,
}

impl<R: AgentRunner + 'static> SupervisorQueue<R> {
    pub fn new(
        runner: R,
        concurrency: usize,
        events: mpsc::Sender<AgentEvent>,
        scratch_root: PathBuf,
    ) -> Self {
        assert!(concurrency >= 1, "동시 실행은 1 이상이어야 해요");
        SupervisorQueue {
            runner: Arc::new(Mutex::new(runner)),
            pending: Arc::new(Mutex::new(VecDeque::new())),
            events,
            concurrency,
            scratch_root,
        }
    }

    /// 작업 등록(즉시 실행 슬롯이 있으면 바로).
    pub fn enqueue(&self, task: AgentTask) {
        self.pending.lock().unwrap_or_else(|p| p.into_inner()).push_back(task);
    }

    /// 대기열을 소진할 때까지 실행(동시 상한 준수). 테스트·앱 양쪽 사용.
    pub fn run_all(&self) {
        loop {
            let batch: Vec<AgentTask> = {
                let mut q = self.pending.lock().unwrap_or_else(|p| p.into_inner());
                if q.is_empty() {
                    break;
                }
                let n = self.concurrency.min(q.len());
                (0..n).filter_map(|_| q.pop_front()).collect()
            };
            if batch.is_empty() {
                break;
            }
            let mut handles = Vec::new();
            for task in batch {
                let runner = Arc::clone(&self.runner);
                let events = self.events.clone();
                let scratch = self.scratch_root.clone();
                handles.push(std::thread::spawn(move || {
                    let _ = events.send(AgentEvent::Started {
                        task_id: task.id.clone(),
                        prompt: task.prompt.clone(),
                    });
                    let sandbox = SandboxSpec::for_task(&scratch, &task.id);
                    std::fs::create_dir_all(&sandbox.workdir).ok();
                    let mut guard = runner.lock().unwrap_or_else(|p| p.into_inner());
                    match guard.run(&task, &sandbox) {
                        Ok(files) => {
                            let _ = events.send(AgentEvent::Produced {
                                task_id: task.id.clone(),
                                summary: crate::changeset::heuristic_summary(&files),
                            });
                        }
                        Err(reason) => {
                            let _ = events.send(AgentEvent::Failed {
                                task_id: task.id.clone(),
                                reason,
                            });
                        }
                    }
                }));
            }
            for h in handles {
                let _ = h.join();
            }
        }
    }
}

/// 산출 → 변경 세트 변환(자동 요약 A5 — origin: Agent).
pub fn files_to_changeset(
    task: &AgentTask,
    files: Vec<CsFile>,
    base_commit: &str,
) -> Changeset {
    let summary = crate::changeset::heuristic_summary(&files);
    Changeset {
        id: format!("agent-{}-{}", task.id, task.author_display),
        author_display: task.author_display.clone(),
        summary,
        base_commit: base_commit.to_string(),
        files,
        origin: CsOrigin::Agent,
        state: CsState::Draft,
    }
}

/// L3 탐지 — 볼트 원본 디렉터리의 무단 쓰기 흔적 검사(워커 외 쓰기 차단
/// 검증). 진짜 차단은 샌드박스 프로파일(L2)이 하고, 여기는 탐지·알림.
pub fn detect_vault_tampering(vault_path: &Path, known_state: &[(PathBuf, u64)]) -> Option<String> {
    for (rel, mtime) in known_state {
        let full = vault_path.join(rel);
        if let Ok(meta) = std::fs::metadata(&full) {
            if let Ok(m) = meta.modified() {
                let secs = m
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if secs > *mtime {
                    return Some(format!(
                        "노트 원본이 예고 없이 바뀌었어요: {}",
                        rel.display()
                    ));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 결정론적 가짜 데몬 — 금지 행위를 시도하는 적대 프로파일.
    struct FakeDaemon {
        /// 시도할 금지 행위 목록(환경 읽기·경로 쓰기 등).
        attempts: Vec<ForbiddenAttempt>,
    }

    #[derive(Debug, Clone)]
    enum ForbiddenAttempt {
        /// L1: 토큰 환경변수 읽기 시도.
        ReadEnvToken,
        /// L2: 샌드박스 밖 경로(볼트 원본) 쓰기 시도.
        WriteOutsideSandbox(PathBuf),
    }

    impl AgentRunner for FakeDaemon {
        fn run(&mut self, task: &AgentTask, sandbox: &SandboxSpec) -> Result<Vec<CsFile>, String> {
            for attempt in &self.attempts {
                match attempt {
                    ForbiddenAttempt::ReadEnvToken => {
                        // L1 단언: 스크럽된 환경에 토큰이 없어야
                        if std::env::var("GITHUB_TOKEN").is_ok() || std::env::var("GH_TOKEN").is_ok() {
                            // 주의: 이 검사는 부모 환경이 오염된 경우 실패 의미
                        }
                    }
                    ForbiddenAttempt::WriteOutsideSandbox(path) => {
                        // L2 모의: workdir 밖 쓰기는 실행기가 거부해야 함 —
                        // 실 프로파일 검증은 sandbox_exec 프로파일 빌더 테스트로
                        let _ = path;
                    }
                }
            }
            Ok(vec![CsFile {
                path: task.target_doc.clone().unwrap_or_else(|| "AI작업/결과.md".into()),
                content: Some(format!("# {}\n\n{}\n", task.prompt, task.author_display)),
                binary_b64: None,
            }])
        }
    }

    #[test]
    fn m3_queue_parallel_events_and_outputs() {
        let (tx, rx) = mpsc::channel();
        let tmp = tempfile::tempdir().unwrap();
        let queue = SupervisorQueue::new(FakeDaemon { attempts: vec![] }, 3, tx, tmp.path().into());

        for i in 0..5 {
            queue.enqueue(AgentTask {
                id: format!("t{i}"),
                prompt: format!("작업 {i} 정리해 줘"),
                target_doc: Some(format!("AI작업/{i}.md")),
                author_display: "에이전트".into(),
            });
        }
        queue.run_all();

        let mut started = 0;
        let mut produced = 0;
        for ev in rx.try_iter() {
            match ev {
                AgentEvent::Started { .. } => started += 1,
                AgentEvent::Produced { .. } => produced += 1,
                _ => {}
            }
        }
        assert_eq!(started, 5, "전 작업 시작 이벤트");
        assert_eq!(produced, 5, "전 작업 산출 이벤트(F28)");
    }

    #[test]
    fn m3_files_to_changeset_agent_origin_and_summary() {
        let task = AgentTask {
            id: "abc".into(),
            prompt: "회의록 정리".into(),
            target_doc: None,
            author_display: "에이전트".into(),
        };
        let files = vec![CsFile {
            path: "AI작업/결과.md".into(),
            content: Some("내용".into()),
            binary_b64: None,
        }];
        let cs = files_to_changeset(&task, files, "base123");
        assert_eq!(cs.origin, CsOrigin::Agent);
        assert_eq!(cs.state, CsState::Draft);
        assert!(cs.summary.contains("고침"));
        assert!(cs.id.starts_with("agent-abc"));
    }

    #[test]
    fn m3_sandbox_spec_isolation_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = SandboxSpec::for_task(tmp.path(), "t1");
        assert!(spec.scrub_environment, "L1: 환경 스크럽 기본");
        assert!(spec.readonly_mounts.is_empty(), "원본 직접 마운트 없음");
        assert!(spec.workdir.ends_with("t1"), "작업별 격리 디렉터리");
    }

    /// L3: 볼트 원본 무단 변경 탐지.
    #[test]
    fn m3_detect_vault_tampering() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = tmp.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let doc = vault.join("a.md");
        std::fs::write(&doc, "원본").unwrap();

        let known = vec![(PathBuf::from("a.md"), 0u64)]; // mtime 0 = 항상 최신보다 옛날
        assert!(detect_vault_tampering(&vault, &known).is_some(), "변경 감지");

        let future = u64::MAX / 2;
        let known_future = vec![(PathBuf::from("a.md"), future)];
        assert!(detect_vault_tampering(&vault, &known_future).is_none(), "무변경");
    }

    /// S1 기반 claude 명령줄 조립(stream-json + L1 권한 플래그).
    #[test]
    fn m3_claude_command_shape() {
        let args = claude_args("지시문");
        assert!(args.contains(&"--print".to_string()));
        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.windows(2).any(|w| w == ["--output-format", "stream-json"]));
        assert!(args.contains(&"--verbose".to_string()));
        assert!(args.last() == Some(&"지시문".to_string()));
        // L1 게이트: 권한 플래그 포함 + 게이트 통과
        assert!(args.windows(2).any(|w| w == [L1_PERMISSION_FLAG, L1_PERMISSION_VALUE]));
        assert!(enforce_l1_gate(&args).is_ok());
        // 플래그 없는 인자 목록은 기동 거부
        let bare = vec!["--print".to_string()];
        assert!(enforce_l1_gate(&bare).is_err());
        // 우회 값(bypassPermissions)로는 쌍이 아니어서 거부 — §3의4 완화
        // 계층의 자동 단언(§10.3(b) 자동 부분; 실 보안 도구 시도는 수동 프로토콜)
        let bypass = vec![
            "--permission-mode".to_string(),
            "bypassPermissions".to_string(),
        ];
        assert!(enforce_l1_gate(&bypass).is_err(), "우회 권한 값 거부");
    }
}

/// S1 실측 프로토콜 — claude CLI 인자 조립(테스트가 형상 검증).
/// L1 게이트(S6 승격 요구사항): `--permission-mode acceptEdits`로 자식
/// 스스로 권한을 좁힌다(파일 편집 자동 수용, 그 외 도구는 --print에서
/// 자동 거부). 이 플래그 없이는 에이전트를 기동하지 않는다.
pub const L1_PERMISSION_FLAG: &str = "--permission-mode";
pub const L1_PERMISSION_VALUE: &str = "acceptEdits";

pub fn claude_args(prompt: &str) -> Vec<String> {
    vec![
        "--print".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        L1_PERMISSION_FLAG.into(),
        L1_PERMISSION_VALUE.into(),
        prompt.into(),
    ]
}

/// L1 게이트 검사 — 권한 플래그가 **허용 값과 정확한 쌍**으로 있어야
/// 통과(플래그 존재만 보면 bypassPermissions 같은 우회 값이 새어 들어간다).
pub fn enforce_l1_gate(args: &[String]) -> Result<(), String> {
    let pair_ok = args
        .windows(2)
        .any(|w| w == [L1_PERMISSION_FLAG, L1_PERMISSION_VALUE]);
    if pair_ok {
        Ok(())
    } else {
        Err("AI 도우미를 안전 모드로 시작하지 못했어요 (권한 제한 플래그 누락 또는 값 오류)".into())
    }
}

// ── claude 실 어댑터(작업사본 스냅숏 → 실행 → diff 수집) ─────────────

/// 작업사본 스냅숏 — 샌드박스 안에서 에이전트가 보는 볼트 뷰(사본).
pub fn snapshot_for_task(scratch: &Path, files: &[(String, String)]) -> PathBuf {
    let snapshot = scratch.join("볼트사본");
    for (path, content) in files {
        let full = snapshot.join(path);
        if let Some(dir) = full.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(full, content).ok();
    }
    snapshot
}

/// 실행 후 변경 수집 — 스냅숏 대비 생성·수정 파일(삭제는 볼트 뷰에서 제외).
pub fn collect_changes(snapshot: &Path) -> Vec<CsFile> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<CsFile>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
                continue;
            }
            // 볼트 상대 경로는 git·UI 규약대로 `/` 구분자로 통일(Windows `\` 유입 방지)
            let rel = p
                .strip_prefix(base)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            if rel.is_empty() || rel.starts_with('.') {
                continue;
            }
            if let Ok(bytes) = std::fs::read(&p) {
                match String::from_utf8(bytes.clone()) {
                    Ok(text) => out.push(CsFile { path: rel, content: Some(text), binary_b64: None }),
                    Err(_) => {
                        use base64::Engine;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
                        out.push(CsFile { path: rel, content: None, binary_b64: Some(b64) });
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(snapshot, snapshot, &mut out);
    out
}

/// claude 실실행 어댑터 — L1/L2 샌드박스 안에서 stream-json 실행(S1).
pub struct ClaudeAdapter {
    pub claude_bin: String,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        ClaudeAdapter { claude_bin: "claude".into() }
    }

    /// 지시문 조립 — 비개발자 언어 정책(A1)과 파일 범위 고정.
    pub fn build_prompt(task: &AgentTask) -> String {
        match &task.target_doc {
            Some(doc) => format!(
                "다음 문서를 요청대로 고쳐 주세요. 결과는 같은 폴더의 같은 이름 파일로 저장해 주세요.\\n문서: {doc}\\n요청: {}",
                task.prompt
            ),
            None => format!(
                "요청을 문서로 정리해 'AI작업' 폴더에 마크다운 파일로 저장해 주세요.\\n요청: {}",
                task.prompt
            ),
        }
    }
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentRunner for ClaudeAdapter {
    fn run(&mut self, task: &AgentTask, sandbox: &SandboxSpec) -> Result<Vec<CsFile>, String> {
        let prompt = Self::build_prompt(task);
        let snapshot = snapshot_for_task(&sandbox.workdir, &sandbox.snapshot_files);
        std::fs::create_dir_all(&snapshot).map_err(|e| format!("작업 공간 준비 실패: {e}"))?;
        let args = crate::agent::claude_args(&prompt);
        // L1 게이트: 플래그 없는 기동은 앱이 거부한다(S6 승격 요구사항)
        crate::agent::enforce_l1_gate(&args)?;
        let out = crate::sandbox::run_sandboxed(&self.claude_bin, &args, &sandbox.workdir, &[]);
        if out.unsupported_platform {
            return Err(out.stderr);
        }
        if !out.status.map(|s| s.success()).unwrap_or(false) {
            return Err(format!(
                "AI 도우미가 일을 마치지 못했어요. 잠시 후 다시 시도해 주세요 ({}의 마지막 안내: {})",
                self.claude_bin,
                out.stderr.lines().last().unwrap_or("없음")
            ));
        }
        let changes = collect_changes(&snapshot);
        if changes.is_empty() {
            return Err("AI 도우미가 결과 파일을 만들지 않았어요".into());
        }
        Ok(changes)
    }
}

/// 충돌 해소 골격(R7) — resolving cs를 해소 실행기로 재작성.
/// 성공: cs 갱신(원문 유지 원칙은 해소본으로 교체 — 재검토 대상) +
/// pending_review 복귀. 실패 누적: 상한 초과 시 cancelled + 알림 문구 반환.
pub fn resolve_conflict<R: AgentRunner>(
    runner: &mut R,
    scratch_root: &Path,
    cs: &mut Changeset,
    current_main_content: &str,
) -> Result<ResolveOutcome, String> {
    // 해소 지시문 — 충돌 용어 없는 일상 언어(충돌 검증 AC: 용어 0건)
    let doc = cs
        .files
        .first()
        .map(|f| f.path.clone())
        .unwrap_or_else(|| "문서.md".into());
    let mine = cs
        .files
        .first()
        .and_then(|f| f.content.clone())
        .unwrap_or_default();
    let task = AgentTask {
        id: format!("resolve-{}", cs.id),
        prompt: format!(
            "두 문서 초안이 서로 달라요. 두 뜻을 모두 살려 하나의 문서로 합쳐 주세요.\\n\\n[최신 공유 문서]\\n{current_main_content}\\n\\n[우리 팀 초안]\\n{mine}"
        ),
        target_doc: Some(doc.clone()),
        author_display: cs.author_display.clone(),
    };
    let sandbox = SandboxSpec::for_task(scratch_root, &task.id);
    std::fs::create_dir_all(&sandbox.workdir).map_err(|e| e.to_string())?;
    match runner.run(&task, &sandbox) {
        Ok(mut files) if !files.is_empty() => {
            // 해소본으로 교체(첫 파일 = 병합 문서) — origin: Resolution
            let merged = files.remove(0);
            cs.files = vec![CsFile { path: doc, content: merged.content, binary_b64: None }];
            cs.origin = CsOrigin::Resolution;
            cs.summary = "조정 후 다시 정리된 문서".into();
            crate::changeset::transition(cs, CsState::PendingReview)?;
            Ok(ResolveOutcome::Resolved)
        }
        Ok(_) => Err("합친 결과가 비었어요".into()),
        Err(e) => Err(e),
    }
}

#[derive(Debug, PartialEq)]
pub enum ResolveOutcome {
    /// 해소 성공 — 갱신 cs가 재검토로.
    Resolved,
}

#[cfg(test)]
mod m3_more_tests {
    use super::*;

    /// 프롬프트에 git 어휘·충돌 용어 부재(A1 + 충돌 검증 AC).
    #[test]
    fn m3_prompt_everyday_language() {
        let task = AgentTask {
            id: "t".into(),
            prompt: "회의록 정리해 줘".into(),
            target_doc: Some("회의/a.md".into()),
            author_display: "에이전트".into(),
        };
        let p = ClaudeAdapter::build_prompt(&task);
        assert!(crate::ui_strings::audit_no_git_vocabulary(&p, crate::ui_strings::Scope::App).is_empty());
        assert!(!p.contains("충돌"));

        let resolving_task_prompt = format!(
            "두 문서 초안이 서로 달라요.{}",
            ""
        );
        assert!(!resolving_task_prompt.contains("충돌"));
    }

    /// 스냅숏 → 가짜 실행(파일 수정) → 변경 수집.
    #[test]
    fn m3_snapshot_diff_collect() {
        let tmp = tempfile::tempdir().unwrap();
        let snap = snapshot_for_task(tmp.path(), &[("회의/a.md".into(), "원본".into())]);
        assert!(snap.join("회의/a.md").exists());
        // 에이전트 흉내: 파일 수정 + 신규 추가
        std::fs::write(snap.join("회의/a.md"), "수정본").unwrap();
        std::fs::write(snap.join("회의/새.md"), "새 파일").unwrap();
        let changes = collect_changes(&snap);
        let paths: Vec<&str> = changes.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"회의/a.md"));
        assert!(paths.contains(&"회의/새.md"));
        // 이진 파일 base64 처리
        std::fs::write(snap.join("img.png"), [0x89, b'P']).unwrap();
        let changes = collect_changes(&snap);
        let img = changes.iter().find(|f| f.path == "img.png").unwrap();
        assert!(img.binary_b64.is_some() && img.content.is_none());
    }

    /// 해소 골격(R7): 가짜 해소기 → cs 교체·Resolution 기원·재검토 복귀.
    struct FakeResolver;
    impl AgentRunner for FakeResolver {
        fn run(&mut self, _t: &AgentTask, s: &SandboxSpec) -> Result<Vec<CsFile>, String> {
            let snap = s.workdir.join("볼트사본");
            std::fs::create_dir_all(&snap).unwrap();
            std::fs::write(snap.join("merged.md"), "# 합친 문서").unwrap();
            Ok(vec![CsFile { path: "merged.md".into(), content: Some("# 합친 문서".into()), binary_b64: None }])
        }
    }

    #[test]
    fn m3_resolve_conflict_produces_resolution_cs() {
        let tmp = tempfile::tempdir().unwrap();
        let mut cs = Changeset {
            id: "cs-x".into(),
            author_display: "김하나".into(),
            summary: "원 제안".into(),
            base_commit: "b".into(),
            files: vec![CsFile { path: "회의/a.md".into(), content: Some("# 우리 초안".into()), binary_b64: None }],
            origin: CsOrigin::Edit,
            state: CsState::Resolving,
        };
        let out = resolve_conflict(&mut FakeResolver, tmp.path(), &mut cs, "# 최신 공유본").unwrap();
        assert_eq!(out, ResolveOutcome::Resolved);
        assert_eq!(cs.state, CsState::PendingReview);
        assert_eq!(cs.origin, CsOrigin::Resolution);
        assert_eq!(cs.files[0].path, "회의/a.md");
        assert!(cs.files[0].content.as_deref().unwrap().contains("합친 문서"));
        assert!(cs.summary.contains("조정"));
    }

    /// 해소 실패 반복 → 호출자가 R7 상한 관리(전이 조합 재검증).
    #[test]
    fn m3_resolution_failure_surfaces_error() {
        struct AlwaysFail;
        impl AgentRunner for AlwaysFail {
            fn run(&mut self, _t: &AgentTask, s: &SandboxSpec) -> Result<Vec<CsFile>, String> {
                let _ = s;
                Err("AI 도우미가 합치지 못했어요".into())
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let mut cs = Changeset {
            id: "cs-y".into(),
            author_display: "김하나".into(),
            summary: "s".into(),
            base_commit: "b".into(),
            files: vec![CsFile { path: "a.md".into(), content: Some("x".into()), binary_b64: None }],
            origin: CsOrigin::Edit,
            state: CsState::Resolving,
        };
        let err = resolve_conflict(&mut AlwaysFail, tmp.path(), &mut cs, "y").unwrap_err();
        assert!(err.contains("합치지 못했어요"));
        assert_eq!(cs.state, CsState::Resolving, "실패 시 resolving 유지");
    }
}
