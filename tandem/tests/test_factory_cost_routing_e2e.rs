mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::sync::Arc;

use common::factory_fixture::{
    FIXED_TODAY, FactoryFixture, FixtureExecutor, StubEnvLoginProbe, TEST_TERMS, orch_actor,
    sys_actor, user_actor, user_caller,
};
use serde_json::Value;
use tandem::adapters::base::{AdapterError, Model};
use tandem::factory::approval::mark_ready;
use tandem::factory::artifacts;
use tandem::factory::broker::context::ContextManifest;
use tandem::factory::broker::job::{JobInput, JobState, job_dir, set_job_state};
use tandem::factory::events::read_all;
use tandem::factory::executor::{BrokerExecutor, SharedModelRunner};
use tandem::factory::machine::Command;
use tandem::factory::model::{FeasibilityVerdict, ModelPrice, Risk, Run, RunState};
use tandem::factory::run::{CreateRunOptions, create_run, get_run};
use tandem::factory::runner::drive_run_step_with_gh;
use tandem::factory::team::{
    AssignStageInput, AssignmentSource, load_assignments, relaunch_agent_with_runner,
    suggest_escalation, terminate_agent,
};
use tandem::tmux::SESSION;
use tandem::tmux::client::TmuxClient;

struct TmuxSessionGuard {
    socket: String,
}

impl TmuxSessionGuard {
    fn start(socket: String) -> Self {
        let status = StdCommand::new("tmux")
            .args([
                "-L",
                &socket,
                "new-session",
                "-d",
                "-s",
                SESSION,
                "-n",
                "status",
                "sleep",
                "120",
            ])
            .status()
            .expect("spawn real tmux session");
        assert!(status.success(), "tmux new-session failed: {status}");
        Self { socket }
    }
}

impl Drop for TmuxSessionGuard {
    fn drop(&mut self) {
        let _ = StdCommand::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .status();
    }
}

fn routed_stubs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub-clis-routed")
        .canonicalize()
        .expect("stub-clis-routed fixture directory")
}

fn model(id: &str, label: &str) -> Model {
    Model {
        id: id.to_string(),
        label: label.to_string(),
    }
}

fn sample_models() -> SharedModelRunner {
    Arc::new(|cli: &str| -> Result<Vec<Model>, AdapterError> {
        match cli {
            "claude" => Ok(vec![
                model("opus", "opus (latest)"),
                model("sonnet", "sonnet (latest)"),
                model("fable", "fable (latest)"),
            ]),
            "agy" => Ok(vec![
                model("gemini-2.0-flash-lite", "Gemini 2.0 Flash Lite"),
                model("gemini-2.0-flash", "Gemini 2.0 Flash"),
                model("gemini-2.5-pro", "Gemini 2.5 Pro"),
            ]),
            "opencode" => Ok(vec![
                model("opencode/glm-4.6-free", "GLM 4.6 (free)"),
                model("anthropic/claude-sonnet-4-6", "Claude Sonnet 4.6"),
            ]),
            other => Err(AdapterError::ModelList(format!("unknown cli {other}"))),
        }
    })
}

fn price(input: f64, output: f64) -> ModelPrice {
    ModelPrice {
        input,
        output,
        as_of: "2026-11-20".to_string(),
    }
}

struct Team {
    fixture: FactoryFixture,
    env: BTreeMap<String, String>,
    socket: String,
    runner: SharedModelRunner,
    _guard: TmuxSessionGuard,
}

impl Team {
    fn start(scenario: &str, signed_out: &str, complexity: Option<&str>) -> Option<Self> {
        if !common::require_tool("tmux") {
            return None;
        }
        let fixture = FactoryFixture::new(scenario);
        let stubs = routed_stubs_dir();
        let mut env = fixture.env_map();
        let old_path = env.get("PATH").cloned().unwrap_or_default();
        env.insert(
            "PATH".to_string(),
            format!("{}:{old_path}", stubs.display()),
        );
        env.insert("STUB_SIGNED_OUT".to_string(), signed_out.to_string());
        env.remove("ANTHROPIC_API_KEY");
        env.remove("GEMINI_API_KEY");

        let mut cfg = fixture.store.settings().unwrap();
        cfg.price_table
            .insert("gemini-2.0-flash".to_string(), price(0.10, 0.40));
        cfg.price_table
            .insert("gemini-2.5-pro".to_string(), price(1.25, 10.0));
        cfg.price_table
            .insert("claude-haiku-4-5".to_string(), price(1.0, 5.0));
        cfg.price_table
            .insert("claude-sonnet-4-6".to_string(), price(3.0, 15.0));
        cfg.price_table
            .insert("claude-opus-4-7".to_string(), price(15.0, 75.0));
        tandem::factory::settings::save(&fixture.store, &cfg).unwrap();

        if let Some(c) = complexity {
            fs::write(fixture.store.root.join("stub-complexity"), c).unwrap();
        }

        let socket = format!(
            "tandem-cost-{}-{}",
            std::process::id(),
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        let guard = TmuxSessionGuard::start(socket.clone());
        Some(Self {
            fixture,
            env,
            socket,
            runner: sample_models(),
            _guard: guard,
        })
    }

    fn probe(&self) -> StubEnvLoginProbe {
        StubEnvLoginProbe {
            env: self.env.clone(),
            signed_out: false,
            stub_only_dir: Some(routed_stubs_dir()),
        }
    }

    fn tmux(&self) -> TmuxClient {
        TmuxClient::new(&self.socket, None)
    }

    fn create_run(&self) -> Run {
        let idea_id = prepare_ready_idea(&self.fixture);
        create_run(
            &self.fixture.store,
            &self.fixture.workspace,
            &idea_id,
            CreateRunOptions::default(),
            &sys_actor(),
        )
        .unwrap()
    }

    fn drive(&self, run_id: &str) -> Run {
        let mut exec = FixtureExecutor {
            broker: BrokerExecutor::new(&self.fixture.workspace)
                .with_tmux(self.tmux())
                .with_login_probe(Arc::new(self.probe()))
                .with_model_runner(Arc::clone(&self.runner)),
            parent_env: self.env.clone(),
            terms: TEST_TERMS,
            today: FIXED_TODAY.to_string(),
        };
        for _ in 0..16 {
            let cur = get_run(&self.fixture.store, run_id).unwrap().unwrap();
            if matches!(
                cur.state,
                RunState::Delivered | RunState::Paused | RunState::Cancelled | RunState::Failed
            ) {
                break;
            }
            drive_run_step_with_gh(
                &self.fixture.store,
                &self.fixture.workspace,
                run_id,
                &mut exec,
                &self.fixture.gh_bin,
            )
            .unwrap();
        }
        let settled = get_run(&self.fixture.store, run_id).unwrap().unwrap();
        assert_eq!(
            settled.state,
            RunState::Delivered,
            "run did not deliver: pause_reason={:?} stages={:?}",
            settled.pause_reason,
            settled.stages
        );
        settled
    }

    fn started(&self) -> BTreeMap<String, Value> {
        let log = read_all(&self.fixture.store.root.join("events.jsonl")).unwrap();
        log.events
            .into_iter()
            .filter(|e| e.r#type == "job.started")
            .map(|e| {
                (
                    format!(
                        "{}#{}",
                        e.data["stage"].as_str().unwrap(),
                        e.data["attempt"]
                    ),
                    e.data,
                )
            })
            .collect()
    }

    fn first(&self, stage: &str) -> Value {
        self.started()
            .remove(&format!("{stage}#1"))
            .unwrap_or_else(|| panic!("no job.started for stage {stage}"))
    }

    fn fabricate_job(
        &self,
        run: &Run,
        stage: &str,
        attempt: u32,
        cli: &str,
        model: &str,
        status: &str,
    ) -> String {
        let jdir = job_dir(&self.fixture.store, &run.id, stage, attempt);
        fs::create_dir_all(&jdir).unwrap();
        fs::write(
            jdir.join("prompt.md"),
            "Cost routing relaunch check STUB_STAY_ALIVE",
        )
        .unwrap();
        let cwd = run
            .worktree_path
            .clone()
            .unwrap_or_else(|| self.fixture.workspace.display().to_string());
        fs::write(
            jdir.join("input.json"),
            serde_json::to_string(&JobInput {
                correlation_id: format!("{}.{stage}.{attempt}", run.id),
                run: run.id.clone(),
                stage: stage.to_string(),
                attempt,
                agent: "senior-backend-dev".to_string(),
                provider: cli.to_string(),
                model: model.to_string(),
                tool_grants: vec![],
                cwd,
                time_limit_secs: 60,
                context_manifest: ContextManifest {
                    readable_scope: Some("worktree".to_string()),
                    ..ContextManifest::default()
                },
            })
            .unwrap(),
        )
        .unwrap();
        fs::write(
            jdir.join("result.json"),
            serde_json::json!({"status": status, "summary": "stub", "files_changed": [], "artifacts": []}).to_string(),
        )
        .unwrap();
        set_job_state(&jdir, JobState::Running).unwrap();
        format!("{}.{stage}.{attempt}", run.id)
    }

    fn job_model(&self, run: &Run, job_id: &str, stage: &str, attempt: u32) -> (String, String) {
        let _ = job_id;
        let raw = fs::read_to_string(
            job_dir(&self.fixture.store, &run.id, stage, attempt).join("input.json"),
        )
        .unwrap();
        let input: JobInput = serde_json::from_str(&raw).unwrap();
        (input.provider, input.model)
    }
}

fn prepare_ready_idea(fixture: &FactoryFixture) -> String {
    let user = user_actor();
    let orch = orch_actor();
    let idea = fixture
        .store
        .create_idea("Cost routing", "Export team activity as CSV", "user", &user)
        .unwrap();
    fixture
        .store
        .execute(
            &idea.id,
            Command::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some("conv-cost-1".to_string()),
            },
            &user,
        )
        .unwrap();
    fixture
        .store
        .execute(
            &idea.id,
            Command::Describe {
                title: "Cost routing".to_string(),
                text: "Export team activity CSV via the cheapest capable models.".to_string(),
            },
            &orch,
        )
        .unwrap();
    let feas_src = fixture.workspace.join(".agent-workspace/feas.md");
    fs::write(&feas_src, "# Feasibility\n\nFeasible.\n").unwrap();
    let feas_ref = artifacts::put(&fixture.store, &idea.id, "feasibility", &feas_src).unwrap();
    fixture
        .store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: Some(FeasibilityVerdict::Feasible),
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();
    fixture
        .store
        .execute(&idea.id, Command::SelectDirection, &orch)
        .unwrap();
    let plan_src = fixture.workspace.join(".agent-workspace/plan.md");
    let plan_body = "## Scope\nExport CSV.\n\n## Acceptance criteria\n- Export team activity to CSV\n\n## Risks\nLow.\n\n## Estimates\n1 day.\n\n## Workflow recommendation\ndefault-delivery\n";
    fs::write(&plan_src, plan_body).unwrap();
    let plan_ref = artifacts::put(&fixture.store, &idea.id, "plan", &plan_src).unwrap();
    fixture
        .store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: artifacts::missing_plan_sections(plan_body),
            },
            &orch,
        )
        .unwrap();
    mark_ready(&fixture.store, &idea.id, &idea.id, "user", &user_caller()).unwrap();
    idea.id
}

fn pair(v: &Value) -> (String, String) {
    (
        v["cli"].as_str().unwrap().to_string(),
        v["model"].as_str().unwrap().to_string(),
    )
}

fn pair_of(cli: &str, model: &str) -> (String, String) {
    (cli.to_string(), model.to_string())
}

#[test]
fn defaults_pick_the_cheapest_capable_model_and_record_provenance() {
    let Some(team) = Team::start("cost-defaults", "", None) else {
        return;
    };
    let run = team.create_run();
    let settled = team.drive(&run.id);

    let plan = team.first("plan");
    let tests = team.first("create-tests");
    let develop = team.first("develop");
    let review = team.first("review");

    assert_eq!(
        pair(&plan),
        pair_of("agy", "gemini-2.0-flash"),
        "price_table makes flash the cheapest Standard model"
    );
    assert_eq!(
        pair(&tests),
        pair_of("opencode", "opencode/glm-4.6-free"),
        "a *-free model is zero cost"
    );
    assert_eq!(pair(&develop), pair_of("agy", "gemini-2.0-flash"));
    assert_eq!(
        pair(&review),
        pair_of("claude", "claude-sonnet-4-6"),
        "review avoids develop's CLI and model"
    );
    assert_ne!(pair(&review), pair(&develop));

    for stage in [&plan, &tests, &develop, &review] {
        assert_eq!(stage["source"], "default");
        assert!(
            stage["selection"].is_object(),
            "provenance missing in {stage}"
        );
    }
    assert_eq!(tests["selection"]["cost_basis"], "free_tier");
    assert_eq!(tests["selection"]["selected_tier"], "economy");
    assert_eq!(develop["selection"]["cost_basis"], "price_table");
    assert_eq!(develop["selection"]["required_tier"], "standard");
    assert!(develop["selection"]["candidates"].as_u64().unwrap() >= 3);
    assert!(
        review["selection"]["rationale"]
            .as_str()
            .unwrap()
            .contains("avoiding")
    );

    let saved = load_assignments(&team.fixture.store, &run.id).unwrap();
    for stage in ["plan", "create-tests", "develop", "review"] {
        let entry = &saved.stages[stage];
        assert_eq!(entry.source, AssignmentSource::Default, "{stage}");
        assert!(
            entry.selection.is_some(),
            "{stage} provenance missing in assignments.json"
        );
    }
    assert_eq!(saved.stages["develop"].model, "gemini-2.0-flash");

    // A failed default attempt escalates one tier on relaunch.
    let tmux = team.tmux();
    let job = team.fabricate_job(&settled, "develop", 98, "agy", "gemini-2.0-flash", "failed");
    let suggestion = suggest_escalation(&team.fixture.store, &run.id, &job, &*team.runner)
        .unwrap()
        .expect("a failed default job has an escalation");
    assert!(suggestion.applies_automatically);
    assert_eq!(suggestion.model, "gemini-2.5-pro");
    relaunch_agent_with_runner(
        &team.fixture.store,
        &tmux,
        &run.id,
        &job,
        team.env.clone(),
        &*team.runner,
    )
    .unwrap();
    assert_eq!(
        team.job_model(&settled, &job, "develop", 98),
        pair_of("agy", "gemini-2.5-pro")
    );
    let escalated = team
        .started()
        .remove("develop#98")
        .expect("job.started for the relaunch");
    assert_eq!(escalated["source"], "default");
    assert_eq!(escalated["selection"]["required_tier"], "premium");
    assert!(
        escalated["selection"]["rationale"]
            .as_str()
            .unwrap()
            .contains("escalated")
    );
    let after = load_assignments(&team.fixture.store, &run.id).unwrap();
    assert_eq!(after.stages["develop"].model, "gemini-2.5-pro");
    assert_eq!(after.stages["develop"].source, AssignmentSource::Default);
    terminate_agent(&team.fixture.store, &tmux, &run.id, &job, &job).unwrap();
}

#[test]
fn high_complexity_slices_raise_the_required_tier() {
    let Some(team) = Team::start("cost-complexity", "", Some("high")) else {
        return;
    };
    let run = team.create_run();
    team.drive(&run.id);

    let plan = team.first("plan");
    assert_eq!(
        plan["selection"]["required_tier"], "standard",
        "no slices exist yet at plan time"
    );
    let develop = team.first("develop");
    assert_eq!(
        pair(&develop),
        pair_of("agy", "gemini-2.5-pro"),
        "cheapest Premium model by price_table"
    );
    assert_eq!(develop["selection"]["required_tier"], "premium");
    assert_eq!(develop["selection"]["selected_tier"], "premium");
    let review = team.first("review");
    assert_eq!(pair(&review), pair_of("claude", "claude-opus-4-7"));
    assert_eq!(review["selection"]["required_tier"], "premium");
}

#[test]
fn user_and_orchestrator_assignments_are_never_overridden_or_escalated() {
    let Some(team) = Team::start("cost-precedence", "", None) else {
        return;
    };
    let run = team.create_run();
    for (stage, cli, model, by, actor) in [
        (
            "create-tests",
            "opencode",
            "anthropic/claude-sonnet-4-6",
            AssignmentSource::Orchestrator,
            "orch-1",
        ),
        (
            "review",
            "claude",
            "sonnet",
            AssignmentSource::User,
            "alice",
        ),
    ] {
        tandem::factory::team::assign_stage(
            &team.fixture.store,
            AssignStageInput {
                run_id: &run.id,
                stage,
                cli,
                model,
                role: None,
                by,
                actor_id: actor,
            },
        )
        .unwrap();
    }
    let settled = team.drive(&run.id);

    let tests = team.first("create-tests");
    assert_eq!(
        pair(&tests),
        pair_of("opencode", "anthropic/claude-sonnet-4-6")
    );
    assert_eq!(tests["source"], "orchestrator");
    assert!(
        tests["selection"].is_null(),
        "assigned stages carry no recommendation provenance"
    );
    let review = team.first("review");
    assert_eq!(pair(&review), pair_of("claude", "sonnet"));
    assert_eq!(review["source"], "user");
    assert_eq!(team.first("develop")["source"], "default");

    let tmux = team.tmux();
    let job = team.fabricate_job(&settled, "review", 97, "claude", "sonnet", "failed");
    let suggestion = suggest_escalation(&team.fixture.store, &run.id, &job, &*team.runner)
        .unwrap()
        .expect("a suggestion is still offered");
    assert!(!suggestion.applies_automatically);
    relaunch_agent_with_runner(
        &team.fixture.store,
        &tmux,
        &run.id,
        &job,
        team.env.clone(),
        &*team.runner,
    )
    .unwrap();
    assert_eq!(
        team.job_model(&settled, &job, "review", 97),
        pair_of("claude", "sonnet"),
        "the user's model is kept"
    );
    let kept = load_assignments(&team.fixture.store, &run.id).unwrap();
    assert_eq!(kept.stages["review"].source, AssignmentSource::User);
    assert_eq!(kept.stages["review"].model, "sonnet");
    assert_eq!(
        kept.stages["create-tests"].source,
        AssignmentSource::Orchestrator
    );
    terminate_agent(&team.fixture.store, &tmux, &run.id, &job, &job).unwrap();
}

#[test]
fn signed_out_clis_are_never_recommended() {
    let Some(team) = Team::start("cost-signed-out", "agy opencode", None) else {
        return;
    };
    let run = team.create_run();
    team.drive(&run.id);

    let started = team.started();
    assert!(!started.is_empty());
    for (key, event) in &started {
        assert_eq!(
            event["cli"], "claude",
            "{key} must not use a signed-out CLI: {event}"
        );
    }
    assert_eq!(
        pair(&team.first("create-tests")),
        pair_of("claude", "claude-haiku-4-5")
    );
    let saved = load_assignments(&team.fixture.store, &run.id).unwrap();
    assert!(saved.stages.values().all(|a| a.cli == "claude"));
}
