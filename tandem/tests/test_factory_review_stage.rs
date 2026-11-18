use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::{SystemTime, UNIX_EPOCH};

use tandem::factory::approval::mark_ready_with_workflow;
use tandem::factory::artifacts;
use tandem::factory::broker::context::{
    ContextInputs, ReviewContextExtras, assemble_review_context,
};
use tandem::factory::broker::gate::{AuthType, consent_provider};
use tandem::factory::broker::result::{JobResult, JobResultStatus};
use tandem::factory::checks::set_checks;
use tandem::factory::events::read_all;
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, FactorySettings, IdeaState, RunState, StageState};
use tandem::factory::routing::{ModelTier, RoutingContext, route};
use tandem::factory::run::get_run;
use tandem::factory::runner::drive_run_step;
use tandem::factory::stages::review::{
    Finding, FindingSeverity, FindingsReport, ReviewRoundsReport, ReviewVerdict, derive_verdict,
};
use tandem::factory::stages::{StageContext, contract_for_stage};
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::parse::parse_workflow_str;
use tandem::factory::workflow::validate::validate;

fn temp_ws(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "tandem-review-stage-{label}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn init_git_repo(dir: &Path) {
    git(dir, &["init", "-b", "trunk"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test"]);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/lib.rs"), "pub fn ok() -> bool { true }\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "init"]);
}

fn setup_review_workflow_run(
    ws: &Path,
    max_review_rounds: Option<u32>,
) -> (FactoryStore, String, String) {
    init_git_repo(ws);
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    if let Some(rounds) = max_review_rounds {
        let mut settings = store.settings().unwrap();
        settings.max_review_rounds = rounds;
        fs::write(
            store.root.join("settings.json"),
            serde_json::to_vec_pretty(&settings).unwrap(),
        )
        .unwrap();
    }
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".into(),
    };
    let idea = store
        .create_idea("Review stage", "Verify review loop", "alice", &user)
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
        "# Plan\n\n## Goal\nRun\n\n## Acceptance Criteria\n- Works\n\n## Files\n- src/lib.rs\n\n## Tests\n- cargo test\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan).unwrap();
    let wf_yaml = r#"name: review-wf
version: 1
trigger: ready-for-development
policy:
  max_parallel_agents: 1
  default_temperature: 0.2
stages:
  - name: develop
    agent: implementer
    tools: [repo_read, repo_write, shell]
  - name: test
    agent: verifier
    depends_on: [develop]
    tools: [test_runner, lint_runner]
  - name: review
    agent: reviewer
    depends_on: [test]
    tools: [repo_read, diff_review]
"#;
    let wf_dir = ws.join(".agent-workspace/factory/workflows");
    fs::create_dir_all(&wf_dir).unwrap();
    fs::write(wf_dir.join("review-wf.yaml"), wf_yaml).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".into(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("review-wf".into()),
                missing_sections: vec![],
            },
            &orch,
        )
        .unwrap();
    let env = |_: &str| None;
    let ready = mark_ready_with_workflow(
        &store, ws, &idea.id, &idea.id, "cli", "alice", None, false, &env,
    )
    .unwrap();
    set_checks(
        &store,
        "workspace",
        vec![vec!["true".into()]],
        vec![],
        Some(30),
        vec![],
        "workspace",
        "alice",
        &env,
    )
    .unwrap();
    consent_provider(&store, "claude", AuthType::ApiKey, "claude", "alice", &env).unwrap();
    (store, idea.id, ready.run.unwrap().id)
}

#[test]
fn test_derive_verdict_blocker_or_major_forces_changes_requested() {
    let mk = |severity| Finding {
        id: "f-1".into(),
        severity,
        file: "src/lib.rs".into(),
        line: 1,
        title: "Issue".into(),
        detail: "Detail".into(),
        suggestion: "Fix it".into(),
    };
    assert_eq!(derive_verdict(&[]), ReviewVerdict::Approve);
    assert_eq!(
        derive_verdict(&[mk(FindingSeverity::Minor), mk(FindingSeverity::Nit)]),
        ReviewVerdict::Approve
    );
    assert_eq!(
        derive_verdict(&[mk(FindingSeverity::Major)]),
        ReviewVerdict::ChangesRequested
    );
    assert_eq!(
        derive_verdict(&[mk(FindingSeverity::Minor), mk(FindingSeverity::Blocker)]),
        ReviewVerdict::ChangesRequested
    );
}

#[test]
fn test_review_contract_validates_findings_and_readonly_worktree() {
    let ws = temp_ws("contract");
    let repo_dir = ws.join("repo");
    let artifact_dir = ws.join("artifacts");
    fs::create_dir_all(&repo_dir).unwrap();
    fs::create_dir_all(&artifact_dir).unwrap();
    init_git_repo(&repo_dir);
    let plan_path = ws.join("plan-snapshot.md");
    fs::write(&plan_path, "# Plan\n").unwrap();

    let result = JobResult {
        status: JobResultStatus::Succeeded,
        summary: "Reviewed".into(),
        rationale: None,
        hypothesis: None,
        usage: None,
        no_behaviour_change: false,
        artifacts: vec!["findings.json".into()],
        files_changed: vec![],
    };
    let contract = contract_for_stage("review").expect("review contract registered");
    let ctx = StageContext {
        run_id: "r-001",
        idea_id: "i-001",
        stage: "review",
        attempt: 1,
        has_repo_write: false,
        artifact_dir: &artifact_dir,
        target_repo: &repo_dir,
        worktree_path: &repo_dir,
        plan_snapshot_path: &plan_path,
        confirmed_checks: None,
        result: &result,
    };

    // Missing findings.json fails
    let err = contract.verify(&ctx).unwrap_err();
    assert_eq!(err.check, "review_findings");

    // Finding referencing file outside repo/diff fails
    let bad_report = FindingsReport {
        verdict: ReviewVerdict::Approve,
        findings: vec![Finding {
            id: "f-1".into(),
            severity: FindingSeverity::Blocker,
            file: "src/nonexistent.rs".into(),
            line: 10,
            title: "Missing".into(),
            detail: "Does not exist".into(),
            suggestion: "Check path".into(),
        }],
    };
    fs::write(
        artifact_dir.join("findings.json"),
        serde_json::to_string_pretty(&bad_report).unwrap(),
    )
    .unwrap();
    let err = contract.verify(&ctx).unwrap_err();
    assert_eq!(err.check, "review_finding_file");

    // Valid finding referencing existing repo file succeeds even if stated verdict differs
    let good_report = FindingsReport {
        verdict: ReviewVerdict::Approve,
        findings: vec![Finding {
            id: "f-1".into(),
            severity: FindingSeverity::Blocker,
            file: "src/lib.rs".into(),
            line: 1,
            title: "Bug".into(),
            detail: "Always returns true".into(),
            suggestion: "Validate input".into(),
        }],
    };
    fs::write(
        artifact_dir.join("findings.json"),
        serde_json::to_string_pretty(&good_report).unwrap(),
    )
    .unwrap();
    contract.verify(&ctx).unwrap();

    // Reviewer editing a file in worktree fails the stage
    fs::write(repo_dir.join("src/lib.rs"), "// modified by reviewer\n").unwrap();
    let err = contract.verify(&ctx).unwrap_err();
    assert_eq!(err.check, "scope_readonly");

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_fresh_reviewer_context_excludes_implementer_summary() {
    let ws = temp_ws("context");
    let plan = ws.join("plan-snapshot.md");
    let playbook = ws.join("playbook.md");
    let agent = ws.join("agent.md");
    fs::write(&plan, "# Plan Snapshot\nImplement feature X.\n").unwrap();
    fs::write(&playbook, "# Review Playbook\nCheck code.\n").unwrap();
    fs::write(&agent, "# Code Reviewer\n").unwrap();

    let base = ContextInputs {
        plan_snapshot_path: plan,
        acceptance_criteria: vec!["Feature X works".into()],
        playbook_path: playbook,
        agent_def_path: agent,
        dependency_summaries: vec![(
            "develop".into(),
            "IMPLEMENTER_CLAIM_SHOULD_NOT_APPEAR".into(),
        )],
        retry_rationale: None,
    };
    let extras = ReviewContextExtras {
        changed_files: vec!["src/lib.rs".into()],
        diff: "+pub fn x() {}".into(),
        evidence: "test: cargo test passed".into(),
    };
    let assembled = assemble_review_context(&base, &extras).unwrap();
    assert!(assembled.prompt.contains("Implement feature X."));
    assert!(assembled.prompt.contains("src/lib.rs"));
    assert!(assembled.prompt.contains("+pub fn x() {}"));
    assert!(assembled.prompt.contains("cargo test passed"));
    assert!(
        !assembled
            .prompt
            .contains("IMPLEMENTER_CLAIM_SHOULD_NOT_APPEAR")
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_routing_prefers_different_provider_or_model_for_reviewer() {
    let raw = parse_workflow_str(
        "name: w\nversion: 1\ntrigger: ready-for-development\nstages:\n  - name: test\n    agent: verifier\n    tools: [test_runner]\n  - name: review\n    agent: reviewer\n    depends_on: [test]\n    tools: [repo_read, diff_review]\n",
    )
    .unwrap();
    let settings = FactorySettings::default();
    let wf = validate(&raw, &settings).unwrap();
    let stage = &wf.stages[1];

    // Two open gates: avoid claude -> picks agy
    let ctx = RoutingContext {
        preferred_provider: Some("claude".into()),
        is_ui_work: false,
        avoid_provider: Some("claude".into()),
        avoid_model: Some("claude-sonnet-4-6".into()),
    };
    let routed = route(stage, &wf, &settings, &["claude", "agy"], 1, &ctx).unwrap();
    assert_eq!(routed.provider, "agy");

    // One open gate: avoid economy model -> upgrades to standard
    let ctx_single = RoutingContext {
        preferred_provider: Some("claude".into()),
        is_ui_work: false,
        avoid_provider: Some("claude".into()),
        avoid_model: Some("claude-haiku-4-5".into()),
    };
    let routed_single = route(stage, &wf, &settings, &["claude"], 1, &ctx_single).unwrap();
    assert_eq!(routed_single.provider, "claude");
    assert_eq!(routed_single.tier, ModelTier::Standard);
    assert_ne!(routed_single.model, "claude-haiku-4-5");
}

struct ScriptedReviewExecutor {
    store: FactoryStore,
    rounds_to_request_changes: u32,
    review_calls: u32,
    develop_inputs: Vec<Option<String>>,
}

impl Executor for ScriptedReviewExecutor {
    fn run(&mut self, job: &StageJob) -> StageOutcome {
        let run = get_run(&self.store, &job.run).unwrap().unwrap();
        let artifact_dir = self
            .store
            .root
            .join("runs")
            .join(&job.run)
            .join("artifacts");
        fs::create_dir_all(&artifact_dir).unwrap();

        if job.stage == "develop" {
            let hyp = run
                .stages
                .iter()
                .find(|s| s.name == "develop")
                .and_then(|s| s.retry_hypothesis.clone());
            self.develop_inputs.push(hyp);
            return StageOutcome::Succeeded;
        }
        if job.stage == "test" {
            return StageOutcome::Succeeded;
        }
        if job.stage == "review" {
            self.review_calls += 1;
            let report = if self.review_calls <= self.rounds_to_request_changes {
                let mut findings = vec![Finding {
                    id: "f-blocker".into(),
                    severity: FindingSeverity::Blocker,
                    file: "src/lib.rs".into(),
                    line: 1,
                    title: "Missing validation".into(),
                    detail: "Handle empty input".into(),
                    suggestion: "Return Err on empty".into(),
                }];
                if self.review_calls == 1 {
                    findings.push(Finding {
                        id: "f-fixed".into(),
                        severity: FindingSeverity::Major,
                        file: "src/lib.rs".into(),
                        line: 1,
                        title: "Dead branch".into(),
                        detail: "Remove unused branch".into(),
                        suggestion: "Delete dead code".into(),
                    });
                }
                // Deliberately state approve on round 1 to test tandem verdict override
                FindingsReport {
                    verdict: ReviewVerdict::Approve,
                    findings,
                }
            } else {
                FindingsReport {
                    verdict: ReviewVerdict::Approve,
                    findings: vec![],
                }
            };
            fs::write(
                artifact_dir.join("findings.json"),
                serde_json::to_string_pretty(&report).unwrap(),
            )
            .unwrap();
            return StageOutcome::Succeeded;
        }
        StageOutcome::Succeeded
    }
}

#[test]
fn test_review_loop_reopens_develop_tracks_rounds_and_approves() {
    let ws = temp_ws("loop-approve");
    let (store, idea_id, run_id) = setup_review_workflow_run(&ws, Some(2));
    let mut exec = ScriptedReviewExecutor {
        store: store.clone(),
        rounds_to_request_changes: 1,
        review_calls: 0,
        develop_inputs: vec![],
    };

    for _ in 0..12 {
        drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    }

    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Review);
    let idea = store.get_idea(&idea_id).unwrap().unwrap();
    assert_eq!(idea.state, IdeaState::Review);
    assert_eq!(exec.review_calls, 2);

    // Second develop run received blocker/major findings as input
    assert_eq!(exec.develop_inputs.len(), 2);
    assert!(exec.develop_inputs[0].is_none());
    let reopened_input = exec.develop_inputs[1].as_deref().unwrap();
    assert!(reopened_input.contains("Missing validation"));
    assert!(reopened_input.contains("Dead branch"));

    // Events include run.changes_requested and stage.reopened
    let events = read_all(&store.root.join("events.jsonl")).unwrap();
    assert!(
        events
            .events
            .iter()
            .any(|e| e.r#type == "run.changes_requested")
    );
    assert!(events.events.iter().any(|e| e.r#type == "stage.reopened"));

    // Every round's findings are kept and show what was fixed
    let artifact_dir = store.root.join("runs").join(&run_id).join("artifacts");
    assert!(artifact_dir.join("findings-round-1.json").exists());
    assert!(artifact_dir.join("findings-round-2.json").exists());
    let rounds: ReviewRoundsReport =
        serde_json::from_str(&fs::read_to_string(artifact_dir.join("review-rounds.json")).unwrap())
            .unwrap();
    assert_eq!(rounds.rounds.len(), 2);
    assert!(rounds.rounds[0].verdict_overridden);
    assert_eq!(
        rounds.rounds[0].derived_verdict,
        ReviewVerdict::ChangesRequested
    );
    assert_eq!(rounds.rounds[1].derived_verdict, ReviewVerdict::Approve);
    assert!(
        rounds.rounds[1]
            .fixed_from_previous
            .contains(&"f-blocker".to_string())
    );
    assert!(
        rounds.rounds[1]
            .fixed_from_previous
            .contains(&"f-fixed".to_string())
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_review_loop_pauses_when_max_review_rounds_exhausted() {
    let ws = temp_ws("loop-exhausted");
    let (store, _idea_id, run_id) = setup_review_workflow_run(&ws, Some(2));
    let mut exec = ScriptedReviewExecutor {
        store: store.clone(),
        rounds_to_request_changes: 5,
        review_calls: 0,
        develop_inputs: vec![],
    };

    for _ in 0..12 {
        drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    }

    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(exec.review_calls, 2);
    assert_eq!(run.state, RunState::Paused);
    let reason = run.pause_reason.as_deref().unwrap_or("");
    assert!(reason.contains("Missing validation"));
    assert!(reason.contains("max_review_rounds"));
    assert!(!run.action_commands.is_empty());

    let review_stage = run.stages.iter().find(|s| s.name == "review").unwrap();
    assert_eq!(review_stage.state, StageState::Failed);

    let artifact_dir = store.root.join("runs").join(&run_id).join("artifacts");
    let rounds: ReviewRoundsReport =
        serde_json::from_str(&fs::read_to_string(artifact_dir.join("review-rounds.json")).unwrap())
            .unwrap();
    assert_eq!(rounds.rounds.len(), 2);
    assert!(
        rounds.rounds[1]
            .fixed_from_previous
            .contains(&"f-fixed".to_string())
    );
    assert!(
        rounds.rounds[1]
            .remaining_from_previous
            .contains(&"f-blocker".to_string())
    );

    let _ = fs::remove_dir_all(&ws);
}
