mod common;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::factory_fixture::{
    FIXED_TODAY, FactoryFixture, FixtureExecutor, StubEnvLoginProbe, TEST_TERMS, TmuxSessionGuard,
    prepare_ready_idea, sys_actor,
};
use tandem::adapters::base::{AdapterError, Model};
use tandem::factory::events::read_all;
use tandem::factory::executor::{BrokerExecutor, SharedModelRunner};
use tandem::factory::model::RunState;
use tandem::factory::run::{CreateRunOptions, create_run, get_run};
use tandem::factory::runner::drive_run_step_with_gh;
use tandem::factory::team::{AssignStageInput, AssignmentSource, assign_stage};
use tandem::tmux::client::TmuxClient;

const AGENT_STAGES: [&str; 4] = ["plan", "create-tests", "develop", "review"];
const FORBIDDEN_ARGS: [&str; 4] = ["-p", "--print", "--output-format", "--bare"];

fn stub_clis_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/stub-clis")
        .canonicalize()
        .expect("stub-clis fixture directory")
}

struct Recorded {
    dir: PathBuf,
    log: PathBuf,
}

impl Recorded {
    fn new(root: &Path) -> Self {
        let dir = root.join("recording-bin");
        fs::create_dir_all(&dir).unwrap();
        let log = root.join("argv.log");
        let real = stub_clis_dir();
        for cli in ["claude", "agy", "opencode"] {
            let script = format!(
                "#!/bin/sh\nprintf '%s\\n' \"{cli} $*\" >> '{}'\nexec '{}/{cli}' \"$@\"\n",
                log.display(),
                real.display()
            );
            let path = dir.join(cli);
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { dir, log }
    }

    fn lines(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn worker_launches(&self) -> Vec<String> {
        self.lines()
            .into_iter()
            .filter(|l| {
                let rest = l.split_once(' ').map_or("", |(_, r)| r);
                !(rest.starts_with("--version")
                    || rest.starts_with("auth ")
                    || rest.starts_with("models")
                    || rest.is_empty())
            })
            .collect()
    }
}

fn models() -> SharedModelRunner {
    Arc::new(|cli: &str| -> Result<Vec<Model>, AdapterError> {
        let list: &[(&str, &str)] = match cli {
            "claude" => &[("opus", "opus"), ("sonnet", "sonnet")],
            "agy" => &[("gemini-2.5-pro", "Gemini 2.5 Pro")],
            "opencode" => &[("anthropic/claude-sonnet-4-6", "Claude Sonnet 4.6")],
            other => return Err(AdapterError::ModelList(format!("unknown cli {other}"))),
        };
        Ok(list
            .iter()
            .map(|(id, label)| Model {
                id: (*id).to_string(),
                label: (*label).to_string(),
            })
            .collect())
    })
}

fn env_with(fixture: &FactoryFixture, recorded: &Recorded) -> BTreeMap<String, String> {
    let mut env = fixture.env_map();
    let old = env.get("PATH").cloned().unwrap_or_default();
    env.insert(
        "PATH".to_string(),
        format!("{}:{old}", recorded.dir.display()),
    );
    env
}

fn assign_all(fixture: &FactoryFixture, run_id: &str) {
    for (stage, cli, model) in [
        ("plan", "agy", "gemini-2.5-pro"),
        ("create-tests", "opencode", "anthropic/claude-sonnet-4-6"),
        ("develop", "claude", "sonnet"),
        ("review", "claude", "sonnet"),
    ] {
        assign_stage(
            &fixture.store,
            AssignStageInput {
                run_id,
                stage,
                cli,
                model,
                role: None,
                by: AssignmentSource::User,
                actor_id: "alice",
            },
        )
        .unwrap();
    }
}

fn executor(
    fixture: &FactoryFixture,
    probe: StubEnvLoginProbe,
    env: BTreeMap<String, String>,
    tmux: TmuxClient,
) -> FixtureExecutor {
    FixtureExecutor {
        broker: BrokerExecutor::new(&fixture.workspace)
            .with_tmux(tmux)
            .with_login_probe(Arc::new(probe))
            .with_model_runner(models()),
        parent_env: env,
        terms: TEST_TERMS,
        today: FIXED_TODAY.to_string(),
    }
}

fn drive(fixture: &FactoryFixture, run_id: &str, exec: &mut FixtureExecutor, steps: usize) {
    for _ in 0..steps {
        let cur = get_run(&fixture.store, run_id).unwrap().unwrap();
        if matches!(
            cur.state,
            RunState::Delivered | RunState::Paused | RunState::Cancelled | RunState::Failed
        ) {
            break;
        }
        drive_run_step_with_gh(
            &fixture.store,
            &fixture.workspace,
            run_id,
            exec,
            &fixture.gh_bin,
        )
        .unwrap();
    }
}

#[test]
fn every_agent_stage_runs_in_its_own_interactive_tmux_pane_and_nothing_runs_headless() {
    if !common::require_tool("tmux") {
        return;
    }
    let fixture = FactoryFixture::new("interactive-only");
    let recorded = Recorded::new(&fixture.workspace.join(".agent-workspace"));
    let env = env_with(&fixture, &recorded);
    let probe = StubEnvLoginProbe {
        env: env.clone(),
        signed_out: false,
        stub_only_dir: Some(recorded.dir.clone()),
    };
    fixture.acknowledge_risky_clis(&probe);

    let socket = format!(
        "tandem-ionly-{}-{}",
        std::process::id(),
        &uuid::Uuid::new_v4().to_string()[..8]
    );
    let _guard = TmuxSessionGuard::start(socket.clone());
    let idea_id = prepare_ready_idea(&fixture, "Interactive only");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();
    assign_all(&fixture, &run.id);

    let mut exec = executor(&fixture, probe, env, TmuxClient::new(&socket, None));
    drive(&fixture, &run.id, &mut exec, 16);

    let settled = get_run(&fixture.store, &run.id).unwrap().unwrap();
    let failures: Vec<String> = read_all(&fixture.store.root.join("events.jsonl"))
        .unwrap()
        .events
        .iter()
        .filter(|e| e.r#type.contains("fail") || e.r#type.contains("block"))
        .map(|e| format!("{} {}", e.r#type, e.data))
        .collect();
    assert_eq!(
        settled.state,
        RunState::Delivered,
        "pause_reason={:?} failures={failures:?}",
        settled.pause_reason
    );

    let log = read_all(&fixture.store.root.join("events.jsonl")).unwrap();
    let started: Vec<_> = log
        .events
        .iter()
        .filter(|e| e.r#type == "job.started")
        .collect();
    let stages: Vec<String> = started
        .iter()
        .map(|e| e.data["stage"].as_str().unwrap_or_default().to_string())
        .collect();
    for stage in AGENT_STAGES {
        assert!(
            stages.iter().any(|s| s == stage),
            "no pane for {stage}: {stages:?}"
        );
    }
    assert!(
        !stages.iter().any(|s| s == "test"),
        "the test stage is tandem-run checks, not an agent pane: {stages:?}"
    );
    let panes: std::collections::BTreeSet<_> = started
        .iter()
        .map(|e| e.data["pane_id"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(panes.len(), started.len(), "one distinct pane per job");
    assert!(panes.iter().all(|p| p.starts_with('%')), "{panes:?}");

    let launches = recorded.worker_launches();
    assert_eq!(
        launches.len(),
        started.len(),
        "every CLI launch is a recorded worker pane, nothing else: {launches:?}"
    );
    for line in recorded.lines() {
        for forbidden in FORBIDDEN_ARGS {
            assert!(
                !line.split_whitespace().any(|a| a == forbidden),
                "headless flag {forbidden} in `{line}`"
            );
        }
        assert!(
            !line.starts_with("opencode run"),
            "headless opencode: {line}"
        );
    }
}

#[test]
fn without_a_tmux_session_the_run_pauses_and_no_cli_is_started() {
    let fixture = FactoryFixture::new("no-tmux");
    let recorded = Recorded::new(&fixture.workspace.join(".agent-workspace"));
    let env = env_with(&fixture, &recorded);
    let probe = StubEnvLoginProbe {
        env: env.clone(),
        signed_out: false,
        stub_only_dir: Some(recorded.dir.clone()),
    };
    fixture.acknowledge_risky_clis(&probe);

    let idea_id = prepare_ready_idea(&fixture, "No tmux");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();
    assign_all(&fixture, &run.id);

    let dead_socket = format!("tandem-no-such-{}", uuid::Uuid::new_v4());
    let mut exec = executor(&fixture, probe, env, TmuxClient::new(&dead_socket, None));
    drive(&fixture, &run.id, &mut exec, 4);

    let paused = get_run(&fixture.store, &run.id).unwrap().unwrap();
    assert_eq!(paused.state, RunState::Paused);
    let reason = paused.pause_reason.unwrap_or_default();
    assert!(
        reason.contains("tmux is required for Factory workers"),
        "{reason}"
    );
    assert!(
        recorded.worker_launches().is_empty(),
        "no CLI may start without tmux: {:?}",
        recorded.worker_launches()
    );
}
