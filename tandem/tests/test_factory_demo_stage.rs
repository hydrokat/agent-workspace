mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tandem::factory::approval::mark_ready_with_workflow;
use tandem::factory::artifacts;
use tandem::factory::broker::browser::{
    BrowserError, ScreenshotRequest, build_screenshot_argv, capture_screenshot,
};
use tandem::factory::broker::gate::{AuthType, consent_provider};
use tandem::factory::broker::result::{JobResult, JobResultStatus};
use tandem::factory::checks::{ChecksConfig, ConfirmedChecks};
use tandem::factory::evidence;
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, RunState, StageState};
use tandem::factory::run::get_run;
use tandem::factory::runner::{compute_ui_changed, drive_run_step};
use tandem::factory::stages::demo::{DemoStageContract, MAX_SCREENSHOT_BYTES, MAX_SCREENSHOTS};
use tandem::factory::stages::{StageContext, StageContract, contract_for_stage};
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::scheduler::TriState;
use tandem::factory::workflow::schema::EvidenceKind;
use tandem::web_deps::{
    DepsEnv, Platform, ScreenshotConsent, load_screenshot_consent, save_screenshot_consent,
    screenshot_install_plan, screenshot_tool_decision, screenshot_tool_status,
};

const PNG_HEADER: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
];

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-demo-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn init_git_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    assert!(
        StdCommand::new("git")
            .args(["init", "-b", "trunk"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    fs::write(dir.join("README.md"), "# repo\n").unwrap();
    assert!(
        StdCommand::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
}

fn run_dir(ws: &Path, run_id: &str) -> PathBuf {
    ws.join(".agent-workspace/factory/runs").join(run_id)
}

fn setup_queued_run(ws: &Path) -> (FactoryStore, String, String) {
    init_git_repo(ws);
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let mut settings = store.settings().unwrap();
    settings.approval_grace_secs = 0;
    tandem::factory::settings::save(&store, &settings).unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".into(),
    };
    let idea = store
        .create_idea("Demo feature", "Add button", "alice", &user)
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch)
        .unwrap();
    let feas = ws.join("f.md");
    fs::write(&feas, "# Feasible\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".into(),
                artifact: feas_ref,
                verdict: None,
                risk: None,
                workflow: None,
                missing_sections: vec![],
            },
            &orch,
        )
        .unwrap();
    store
        .execute(&idea.id, Command::SelectDirection, &orch)
        .unwrap();
    let plan = ws.join("p.md");
    fs::write(
        &plan,
        "# Plan\n\n## Goal\nRun\n\n## Acceptance Criteria\n- Works\n\n## Files\n- a\n\n## Tests\n- test\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".into(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".into()),
                missing_sections: vec![],
            },
            &orch,
        )
        .unwrap();
    let caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);
    consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &caller,
    )
    .unwrap();
    tandem::factory::checks::set_checks(
        &store,
        "workspace",
        vec![vec!["true".to_string()]],
        vec![vec!["true".to_string()]],
        Some(60),
        vec![],
        "workspace",
        "alice",
        &caller,
    )
    .unwrap();
    let ready = mark_ready_with_workflow(
        &store, ws, &idea.id, &idea.id, "alice", None, false, &caller,
    )
    .unwrap();
    (store, idea.id, ready.run.unwrap().id)
}

fn write_slices(artifact_dir: &Path, ui: bool, criteria: &[&str]) {
    let criteria_json = serde_json::to_string(criteria).unwrap();
    let json = format!(
        r#"{{"phase":"p-001-demo","slices":[{{"id":"t-001","title":"UI","type":"New Feature","paths":["src/ui/button.tsx"],"dependencies":[],"acceptance_criteria":{criteria_json},"ui":{ui}}}]}}"#
    );
    fs::create_dir_all(artifact_dir).unwrap();
    fs::write(artifact_dir.join("slices.json"), json).unwrap();
}

fn valid_png_bytes() -> Vec<u8> {
    let mut bytes = PNG_HEADER.to_vec();
    bytes.extend_from_slice(b"IHDR");
    bytes
}

struct StubExecutor {
    stages_run: Vec<String>,
}

impl Executor for StubExecutor {
    fn ready_clis(&self) -> Vec<tandem::factory::executor::ReadyCli> {
        vec![tandem::factory::executor::ReadyCli::new(
            "claude",
            Some(tandem::factory::broker::terms::AuthType::ApiKey),
        )]
    }

    fn run(&mut self, job: &StageJob) -> StageOutcome {
        self.stages_run.push(job.stage.clone());
        StageOutcome::Succeeded
    }
}

#[test]
fn compute_ui_changed_checks_develop_state_ui_slices_and_diff_ui_paths() {
    let ws = temp_ws("ui-changed");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    let art_dir = run_dir(&ws, &run_id).join("artifacts");
    write_slices(&art_dir, false, &["Works"]);

    let mut settings = store.settings().unwrap();
    settings.checks.insert(
        "workspace".to_string(),
        ChecksConfig {
            test: vec![vec!["true".to_string()]],
            lint: vec![vec!["true".to_string()]],
            timeout_secs: 60,
            ui_paths: vec!["src/ui/**".to_string(), "*.css".to_string()],
            confirmed_by: Some("alice".to_string()),
            confirmed_at: Some(1.0),
        },
    );
    let _ = ConfirmedChecks::from_config("workspace", &settings.checks["workspace"]).unwrap();

    let wt = ws.join("wt");
    init_git_repo(&wt);
    let base_out = StdCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&wt)
        .output()
        .unwrap();
    let base_sha = String::from_utf8_lossy(&base_out.stdout).trim().to_string();

    let mut run = get_run(&store, &run_id).unwrap().unwrap();
    run.worktree_path = Some(wt.to_string_lossy().to_string());
    run.base_commit = Some(base_sha);
    run.target = Some("workspace".to_string());

    assert_eq!(
        compute_ui_changed(&store, &run, &settings),
        TriState::Unknown
    );

    for st in &mut run.stages {
        if st.name == "develop" {
            st.state = StageState::Succeeded;
        }
    }
    assert_eq!(compute_ui_changed(&store, &run, &settings), TriState::No);

    fs::create_dir_all(wt.join("src/ui")).unwrap();
    fs::write(
        wt.join("src/ui/App.tsx"),
        "export const App = () => null;\n",
    )
    .unwrap();
    StdCommand::new("git")
        .args(["add", "."])
        .current_dir(&wt)
        .status()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "add ui"])
        .current_dir(&wt)
        .status()
        .unwrap();
    assert_eq!(compute_ui_changed(&store, &run, &settings), TriState::Yes);

    let mut run_no_diff = get_run(&store, &run_id).unwrap().unwrap();
    run_no_diff.stages = run.stages.clone();
    write_slices(&art_dir, true, &["Button renders"]);
    assert_eq!(
        compute_ui_changed(&store, &run_no_diff, &settings),
        TriState::Yes
    );
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn runner_skips_demo_when_ui_changed_is_false() {
    let ws = temp_ws("skip-no-ui");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    let art_dir = run_dir(&ws, &run_id).join("artifacts");
    write_slices(&art_dir, false, &["Backend only"]);

    let mut exec = StubExecutor { stages_run: vec![] };
    for _ in 0..5 {
        drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    }

    let after = get_run(&store, &run_id).unwrap().unwrap();
    let demo = after.stages.iter().find(|s| s.name == "demo").unwrap();
    assert_eq!(demo.state, StageState::Skipped);
    assert!(
        demo.rationale
            .as_deref()
            .is_some_and(|r| r.contains("ui_changed is false")),
        "unexpected rationale: {:?}",
        demo.rationale
    );
    assert!(!exec.stages_run.contains(&"demo".to_string()));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn runner_skips_demo_when_screenshot_tool_missing_or_declined_and_records_evidence() {
    let ws = temp_ws("skip-no-tool");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    let art_dir = run_dir(&ws, &run_id).join("artifacts");
    write_slices(&art_dir, true, &["Button renders"]);

    save_screenshot_consent(
        &ws.join(".agent-workspace/factory"),
        ScreenshotConsent::Declined,
    )
    .unwrap();

    let mut exec = StubExecutor { stages_run: vec![] };
    for _ in 0..8 {
        drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    }

    let after = get_run(&store, &run_id).unwrap().unwrap();
    assert_ne!(after.state, RunState::Failed);
    let demo = after.stages.iter().find(|s| s.name == "demo").unwrap();
    assert_eq!(demo.state, StageState::Skipped);
    assert_eq!(
        demo.rationale.as_deref(),
        Some("screenshot tool not installed")
    );
    assert!(!exec.stages_run.contains(&"demo".to_string()));

    let items = evidence::list(&store, &run_id).unwrap();
    assert!(
        items
            .iter()
            .any(|i| i.stage == "demo" && i.summary.contains("no screenshots were taken")),
        "expected evidence noting no screenshots were taken, got: {items:?}"
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn screenshot_tool_decision_and_consent_follow_lts_and_web_deps_pattern() {
    let decision = screenshot_tool_decision();
    assert_eq!(decision.tool, "chromium");
    assert_eq!(decision.channel, "stable");
    assert!(decision.rationale.contains("LTS"));

    let linux_plan = screenshot_install_plan(
        &Platform {
            os: "linux".into(),
            arch: "x64".into(),
            libc: Some("glibc".into()),
        },
        &["apt".into()],
    );
    assert_eq!(linux_plan.dep, "chromium");
    assert_eq!(linux_plan.commands, vec!["sudo apt install chromium"]);

    let dir = temp_ws("consent");
    assert_eq!(load_screenshot_consent(&dir), ScreenshotConsent::Unasked);
    save_screenshot_consent(&dir, ScreenshotConsent::Declined).unwrap();
    assert_eq!(load_screenshot_consent(&dir), ScreenshotConsent::Declined);

    let env = DepsEnv {
        path: String::new(),
        home: dir.clone(),
        bun_install: None,
    };
    let status = screenshot_tool_status(&env, &dir);
    assert!(!status.installed);
    assert!(!status.available_for_demo());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn browser_broker_enforces_allowlisted_args_and_local_urls() {
    let dir = temp_ws("browser-args");
    let out_png = dir.join("shot.png");

    let req = ScreenshotRequest {
        url: "http://127.0.0.1:3000/dashboard".to_string(),
        output_path: out_png.clone(),
        width: 1280,
        height: 720,
    };
    let argv = build_screenshot_argv(&req).unwrap();
    assert!(argv.contains(&"--headless".to_string()));
    assert!(argv.contains(&"--disable-gpu".to_string()));
    assert!(argv.contains(&"--window-size=1280,720".to_string()));
    assert!(argv.iter().any(|a| a.starts_with("--screenshot=")));
    assert_eq!(argv.last().unwrap(), "http://127.0.0.1:3000/dashboard");

    let remote = ScreenshotRequest {
        url: "https://example.com/".to_string(),
        output_path: out_png.clone(),
        width: 1280,
        height: 720,
    };
    assert!(matches!(
        build_screenshot_argv(&remote),
        Err(BrowserError::InvalidInput(_))
    ));

    let bad_ext = ScreenshotRequest {
        url: "http://localhost:3000/".to_string(),
        output_path: dir.join("shot.jpg"),
        width: 1280,
        height: 720,
    };
    assert!(matches!(
        build_screenshot_argv(&bad_ext),
        Err(BrowserError::InvalidInput(_))
    ));

    let bad_bin = capture_screenshot(
        Path::new("/usr/bin/curl"),
        &req,
        &dir,
        std::time::Duration::from_secs(5),
    );
    assert!(matches!(bad_bin, Err(BrowserError::InvalidInput(_))));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn demo_stage_contract_validates_png_size_count_captions_and_local_evidence() {
    let ws = temp_ws("demo-contract");
    let (store, idea_id, run_id) = setup_queued_run(&ws);
    let art_dir = run_dir(&ws, &run_id).join("artifacts");
    write_slices(&art_dir, true, &["Button renders", "Click opens modal"]);

    let wt = ws.join("wt");
    init_git_repo(&wt);

    let contract = contract_for_stage("demo").expect("demo contract registered");
    assert_eq!(contract.name(), "demo");

    fs::create_dir_all(art_dir.join("demo")).unwrap();
    fs::write(art_dir.join("demo/shot-1.png"), valid_png_bytes()).unwrap();
    fs::write(art_dir.join("demo/shot-2.png"), valid_png_bytes()).unwrap();
    fs::write(
        art_dir.join("demo.json"),
        r#"{"screenshots":[
            {"path":"demo/shot-1.png","caption":"Button rendered in toolbar"},
            {"path":"demo/shot-2.png","caption":"Modal open after click"}
        ]}"#,
    )
    .unwrap();

    let result = JobResult {
        status: JobResultStatus::Succeeded,
        summary: "Captured 2 screenshots".to_string(),
        files_changed: vec![],
        artifacts: vec![
            "demo.json".to_string(),
            "demo/shot-1.png".to_string(),
            "demo/shot-2.png".to_string(),
        ],
        rationale: None,
        hypothesis: None,
        no_behaviour_change: false,
        usage: None,
    };
    let ctx = StageContext {
        run_id: &run_id,
        idea_id: &idea_id,
        stage: "demo",
        attempt: 1,
        has_repo_write: false,
        artifact_dir: &art_dir,
        target_repo: &wt,
        worktree_path: &wt,
        plan_snapshot_path: &art_dir.join("plan.md"),
        confirmed_checks: None,
        result: &result,
    };
    DemoStageContract.verify(&ctx).unwrap();

    let sys = Actor {
        kind: ActorKind::System,
        id: "test".to_string(),
    };
    let recorded =
        evidence::record_demo_screenshots(&store, &idea_id, &run_id, 1, &art_dir, &sys).unwrap();
    assert_eq!(recorded.len(), 2);
    assert!(recorded.iter().all(|e| e.kind == EvidenceKind::Screenshot));

    fs::write(
        art_dir.join("demo.json"),
        r#"{"screenshots":[{"path":"demo/shot-1.png","caption":"Only one"}]}"#,
    )
    .unwrap();
    assert!(DemoStageContract.verify(&ctx).is_err());

    fs::write(
        art_dir.join("demo.json"),
        r#"{"screenshots":[
            {"path":"demo/shot-1.png","caption":"   "},
            {"path":"demo/shot-2.png","caption":"Modal open"}
        ]}"#,
    )
    .unwrap();
    assert!(DemoStageContract.verify(&ctx).is_err());

    fs::write(art_dir.join("demo/shot-2.png"), b"not a png").unwrap();
    fs::write(
        art_dir.join("demo.json"),
        r#"{"screenshots":[
            {"path":"demo/shot-1.png","caption":"One"},
            {"path":"demo/shot-2.png","caption":"Two"}
        ]}"#,
    )
    .unwrap();
    assert!(DemoStageContract.verify(&ctx).is_err());

    let mut oversized = valid_png_bytes();
    oversized.resize(MAX_SCREENSHOT_BYTES + 1, 0);
    fs::write(art_dir.join("demo/shot-2.png"), oversized).unwrap();
    assert!(DemoStageContract.verify(&ctx).is_err());

    fs::write(art_dir.join("demo/shot-2.png"), valid_png_bytes()).unwrap();
    let entries: Vec<serde_json::Value> = (0..=MAX_SCREENSHOTS)
        .map(|i| {
            serde_json::json!({
                "path": "demo/shot-1.png",
                "caption": format!("Shot {i}")
            })
        })
        .collect();
    fs::write(
        art_dir.join("demo.json"),
        serde_json::to_string(&serde_json::json!({ "screenshots": entries })).unwrap(),
    )
    .unwrap();
    assert!(DemoStageContract.verify(&ctx).is_err());

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn validate_local_url_rejects_userinfo_ssrf_bypasses() {
    for bad in [
        "http://localhost:80@evil.com/",
        "http://127.0.0.1:x@169.254.169.254/",
        "https://user@127.0.0.1:3000/",
        "http://127.0.0.1.evil.com/",
        "http://localhost:abc/",
        "http://localhost:/",
    ] {
        let req = ScreenshotRequest {
            url: bad.to_string(),
            output_path: PathBuf::from("shot.png"),
            width: 1280,
            height: 720,
        };
        let err = build_screenshot_argv(&req).unwrap_err();
        assert!(
            matches!(err, BrowserError::InvalidInput(_)),
            "expected InvalidInput for {bad}, got {err:?}"
        );
    }

    for good in [
        "http://127.0.0.1:3000/app?x=1#y",
        "http://localhost:8080",
        "https://[::1]:8443/home",
        "http://[::1]/",
    ] {
        let req = ScreenshotRequest {
            url: good.to_string(),
            output_path: PathBuf::from("shot.png"),
            width: 1280,
            height: 720,
        };
        assert!(
            build_screenshot_argv(&req).is_ok(),
            "expected {good} to be accepted"
        );
    }
}
