use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use tandem::config::Config;
use tandem::factory::commands::run_with_io;
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::model::{Actor, ActorKind, FactorySettings, Risk};
use tandem::factory::simulate::{
    FakeExecutor, SimulationOutcome, SimulationScript, format_trace_text, simulate,
};
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::load_default_workflow;
use tandem::factory::workflow::validate::validate;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-sim-{}-{}-{}",
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

fn dir_snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    fn walk(base: &Path, cur: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(entries) = fs::read_dir(cur) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else if let Ok(bytes) = fs::read(&p) {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().to_string();
                out.push((rel, bytes));
            }
        }
    }
    walk(root, root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn call_factory(
    args: &[&str],
    ws: &Path,
    cfg: &Config,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
) -> i32 {
    let mut full = vec!["tandem"];
    full.extend_from_slice(args);
    let cli = tandem::cli::parse_from(full).unwrap();
    let tandem::cli::Command::Factory(f_args) = cli.command.unwrap() else {
        panic!("expected factory command");
    };
    let mut stdin = std::io::empty();
    run_with_io(&f_args, ws, cfg, &mut stdin, stdout, stderr)
}

#[test]
fn fake_executor_fails_first_n_attempts_then_succeeds() {
    let mut fails = HashMap::new();
    fails.insert("test".to_string(), 2);
    let mut exec = FakeExecutor::new(fails);

    let j1 = StageJob {
        run: "r-1".into(),
        stage: "test".into(),
        attempt: 1,
    };
    let j2 = StageJob {
        run: "r-1".into(),
        stage: "test".into(),
        attempt: 2,
    };
    let j3 = StageJob {
        run: "r-1".into(),
        stage: "test".into(),
        attempt: 3,
    };
    assert!(matches!(exec.run(&j1), StageOutcome::Failed { .. }));
    assert!(matches!(exec.run(&j2), StageOutcome::Failed { .. }));
    assert_eq!(exec.run(&j3), StageOutcome::Succeeded);
}

#[test]
fn simulate_covers_normal_ui_deploy_approval_and_failure() {
    let ws = temp_ws("cases");
    let (raw, _) = load_default_workflow(&ws).unwrap();
    let settings = FactorySettings::default();
    let wf = validate(&raw, &settings).unwrap();

    // 1. Normal run (no ui, no deploy) -> 5 succeeded, 2 skipped
    let t_normal = simulate(&wf, "embedded", &settings, &SimulationScript::default());
    assert_eq!(
        t_normal.outcome,
        SimulationOutcome::Finished {
            succeeded: 5,
            skipped: 2
        }
    );
    let text = format_trace_text(&t_normal);
    assert!(text.contains("finished: 5 stages succeeded, 2 skipped"));

    // 2. UI changed -> 6 succeeded, 1 skipped
    let t_ui = simulate(
        &wf,
        "embedded",
        &settings,
        &SimulationScript {
            ui_changed: true,
            ..Default::default()
        },
    );
    assert_eq!(
        t_ui.outcome,
        SimulationOutcome::Finished {
            succeeded: 6,
            skipped: 1
        }
    );

    // 3. Deploy requested without approval -> Blocked
    let t_blocked = simulate(
        &wf,
        "embedded",
        &settings,
        &SimulationScript {
            deploy_requested: true,
            approve_deploy: false,
            ..Default::default()
        },
    );
    assert!(matches!(
        t_blocked.outcome,
        SimulationOutcome::Blocked { ref stage, .. } if stage == "deploy"
    ));

    // 4. Deploy requested with approval -> 6 succeeded, 1 skipped
    let t_deploy = simulate(
        &wf,
        "embedded",
        &settings,
        &SimulationScript {
            deploy_requested: true,
            approve_deploy: true,
            ..Default::default()
        },
    );
    assert_eq!(
        t_deploy.outcome,
        SimulationOutcome::Finished {
            succeeded: 6,
            skipped: 1
        }
    );

    // 5. Failing stage exhausts retries -> Stopped
    let mut fail_counts = HashMap::new();
    fail_counts.insert("test".to_string(), 3);
    let t_fail = simulate(
        &wf,
        "embedded",
        &settings,
        &SimulationScript {
            fail_counts,
            max_retries: Some(2),
            ..Default::default()
        },
    );
    assert!(matches!(
        t_fail.outcome,
        SimulationOutcome::Stopped { ref stage, .. } if stage == "test"
    ));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn cli_simulate_writes_nothing_and_works_without_factory_mode() {
    let ws = temp_ws("cli");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "user".into(),
    };
    let idea = store
        .create_idea("Sim test", "Check read-only", "user", &user)
        .unwrap();
    let _ = Risk::Low;

    let before = dir_snapshot(&ws.join(".agent-workspace/factory"));

    let cfg = Config {
        factory: false,
        state_dir: ws.join("state"),
        ..Default::default()
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    // --workflow default-delivery --json without factory mode
    let code = call_factory(
        &[
            "factory",
            "run",
            "simulate",
            "--workflow",
            "default-delivery",
            "--json",
        ],
        &ws,
        &cfg,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    let v: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(v["workflow"], "default-delivery");

    // IDEA positional
    stdout.clear();
    let code = call_factory(
        &["factory", "run", "simulate", &idea.id, "--ui-changed"],
        &ws,
        &cfg,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    let out_str = String::from_utf8_lossy(&stdout);
    assert!(out_str.contains("finished: 6 stages succeeded, 1 skipped"));

    // Failing simulation exits 1
    stdout.clear();
    let code = call_factory(
        &[
            "factory",
            "run",
            "simulate",
            "--workflow",
            "default-delivery",
            "--fail",
            "test:3",
            "--retries",
            "2",
        ],
        &ws,
        &cfg,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 1);

    let after = dir_snapshot(&ws.join(".agent-workspace/factory"));
    assert_eq!(before, after, "simulator must not write any files");

    let _ = fs::remove_dir_all(&ws);
}
