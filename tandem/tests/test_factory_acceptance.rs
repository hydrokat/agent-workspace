mod common;

use std::fs;
use std::process::Command as StdCommand;
use std::time::{Duration, Instant};

use common::factory_fixture::{FactoryFixture, orch_actor, sys_actor, user_actor, user_caller};
use tandem::coordination::CoordStore;
use tandem::factory::approval::mark_ready;
use tandem::factory::artifacts;
use tandem::factory::events::read_all;
use tandem::factory::evidence;
use tandem::factory::machine::Command;
use tandem::factory::model::{
    ActorKind, Budget, FeasibilityVerdict, IdeaState, Risk, RunState, StageState,
};
use tandem::factory::permissions;
use tandem::factory::run::{CreateRunOptions, create_run, get_run};
use tandem::factory::runner::{cancel_run, pause_run_by_user, resume_paused_run_with_probe};
use tandem::factory::workflow::parse::load_workflow_file;
use tandem::factory::workflow::schema::WorkflowTool;
use tandem::factory::workflow::validate::validate;
use tandem::snapshot;

fn setup_ready_idea(fixture: &FactoryFixture, title: &str) -> String {
    let user = user_actor();
    let orch = orch_actor();

    let idea = fixture
        .store
        .create_idea(
            title,
            "Export team activity as CSV with date range filtering",
            "user",
            &user,
        )
        .unwrap();

    fixture
        .store
        .execute(
            &idea.id,
            Command::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some("conv-accept-1".to_string()),
            },
            &user,
        )
        .unwrap();

    fixture
        .store
        .execute(
            &idea.id,
            Command::Describe {
                title: title.to_string(),
                text: "Export team activity as CSV filtered by --from and --to dates.".to_string(),
            },
            &orch,
        )
        .unwrap();

    let feas_src = fixture.workspace.join(".agent-workspace/feas.md");
    fs::write(&feas_src, "# Feasibility\n\nFeasible with existing CLI.\n").unwrap();
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
        .execute(&idea.id, Command::SelectDirection, &user)
        .unwrap();

    let plan_src = fixture.workspace.join(".agent-workspace/plan.md");
    let plan_body = "\
# Plan

## Scope
Add `src/export.sh` to export team activity as CSV with `--from` and `--to` date filters.

## Acceptance criteria
- Export team activity to CSV
- Filter exported activity by date range

## Risks
Low risk; isolated shell script.

## Estimates
1 day.

## Workflow recommendation
default-delivery
";
    fs::write(&plan_src, plan_body).unwrap();
    let plan_ref = artifacts::put(&fixture.store, &idea.id, "plan", &plan_src).unwrap();
    let missing = artifacts::missing_plan_sections(plan_body);
    assert!(missing.is_empty());
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
                missing_sections: missing,
            },
            &orch,
        )
        .unwrap();

    mark_ready(&fixture.store, &idea.id, &idea.id, "user", &user_caller()).unwrap();

    idea.id
}

fn drive_until_settled(fixture: &FactoryFixture, run_id: &str, max_steps: usize) -> RunState {
    for _ in 0..max_steps {
        let run = get_run(&fixture.store, run_id)
            .unwrap()
            .unwrap_or_else(|| panic!("run {run_id} disappeared"));
        if matches!(
            run.state,
            RunState::Delivered | RunState::Paused | RunState::Cancelled | RunState::Failed
        ) {
            return run.state;
        }
        fixture.drive_step(run_id).unwrap();
    }
    get_run(&fixture.store, run_id).unwrap().unwrap().state
}

fn wait_for_stage_running(fixture: &FactoryFixture, run_id: &str, stage_name: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(Some(run)) = get_run(&fixture.store, run_id)
            && run
                .stages
                .iter()
                .any(|s| s.name == stage_name && s.state == StageState::Running)
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for stage {stage_name} to enter Running"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Criterion 1: "The user never needs to manually assign delivery agents after the readiness tag."
#[test]
fn criterion_1_no_manual_agent_assignment_after_ready() {
    let fixture = FactoryFixture::new("happy");
    let idea_id = setup_ready_idea(&fixture, "Criterion 1 automated delivery");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    let final_state = drive_until_settled(&fixture, &run.id, 16);
    let final_run = get_run(&fixture.store, &run.id).unwrap().unwrap();
    assert_eq!(
        final_state,
        RunState::Delivered,
        "pause_reason={:?} stages={:?}",
        final_run.pause_reason,
        final_run.stages
    );

    let log = read_all(&fixture.store.root.join("events.jsonl")).unwrap();
    let ready_pos = log
        .events
        .iter()
        .position(|ev| ev.idea == idea_id && ev.r#type == "idea.ready")
        .expect("idea.ready event must be recorded");

    for ev in &log.events[(ready_pos + 1)..] {
        if ev.actor.kind == ActorKind::User {
            assert_eq!(
                ev.r#type, "idea.plan_snapshot_frozen",
                "unexpected user action after idea ready: {}",
                ev.r#type
            );
        }
    }

    let final_run = get_run(&fixture.store, &run.id).unwrap().unwrap();
    for stage in final_run
        .stages
        .iter()
        .filter(|s| s.state == StageState::Succeeded)
    {
        assert!(
            !stage.agent.is_empty(),
            "stage {} should have an automatically routed agent",
            stage.name
        );
    }
}

/// Criterion 2: "Every stage exposes a durable state, timestamp, owner or agent, artifact, and success or failure rationale."
#[test]
fn criterion_2_every_stage_is_durable_and_explained() {
    for (scenario, expected_run_state) in [
        ("happy", RunState::Delivered),
        ("retries-exhausted", RunState::Paused),
    ] {
        let fixture = FactoryFixture::new(scenario);
        let idea_id = setup_ready_idea(&fixture, &format!("Criterion 2 ({scenario})"));
        let run = create_run(
            &fixture.store,
            &fixture.workspace,
            &idea_id,
            CreateRunOptions::default(),
            &sys_actor(),
        )
        .unwrap();

        let final_state = drive_until_settled(&fixture, &run.id, 20);
        assert_eq!(final_state, expected_run_state);

        let original_run = get_run(&fixture.store, &run.id).unwrap().unwrap();
        let original_idea = fixture.store.get_idea(&idea_id).unwrap().unwrap();

        let executed_stages: Vec<_> = original_run
            .stages
            .iter()
            .filter(|s| matches!(s.state, StageState::Succeeded | StageState::Failed))
            .collect();
        assert!(
            !executed_stages.is_empty(),
            "expected executed stages in {scenario}"
        );

        for stage in &executed_stages {
            assert!(
                stage.started.is_some(),
                "stage {} missing start timestamp in {scenario}",
                stage.name
            );
            assert!(
                stage.finished.is_some(),
                "stage {} missing end timestamp in {scenario}",
                stage.name
            );
            assert!(
                !stage.agent.is_empty(),
                "stage {} missing agent in {scenario}",
                stage.name
            );
            let has_artifact_or_evidence = !stage.artifacts.is_empty()
                || original_run.evidence.iter().any(|e| e.stage == stage.name)
                || (stage.name == "plan"
                    && fixture
                        .store
                        .run_dir(&run.id)
                        .join("artifacts/slices.json")
                        .is_file());
            assert!(
                has_artifact_or_evidence,
                "stage {} has neither artifact nor evidence item in {scenario}",
                stage.name
            );
            if stage.state == StageState::Failed {
                assert!(
                    stage
                        .rationale
                        .as_deref()
                        .is_some_and(|r| !r.trim().is_empty()),
                    "failed stage {} missing rationale in {scenario}",
                    stage.name
                );
            }
        }

        if expected_run_state == RunState::Paused {
            assert!(
                original_run
                    .stages
                    .iter()
                    .any(|s| s.state == StageState::Failed),
                "retries-exhausted run must record at least one failed stage"
            );
        }

        let run_json = fixture.store.run_dir(&run.id).join("run.json");
        let idea_json = fixture
            .store
            .root
            .join("ideas")
            .join(format!("{idea_id}.json"));
        fs::remove_file(&run_json).unwrap();
        fs::remove_file(&idea_json).unwrap();
        assert!(!run_json.exists());
        assert!(!idea_json.exists());

        fixture.store.rebuild().unwrap();

        let rebuilt_run = get_run(&fixture.store, &run.id).unwrap().unwrap();
        let rebuilt_idea = fixture.store.get_idea(&idea_id).unwrap().unwrap();
        assert_eq!(rebuilt_run, original_run);
        assert_eq!(rebuilt_idea, original_idea);
    }
}

/// Criterion 3: "A budget breach or policy denial pauses the run and creates a clear action request."
#[test]
fn criterion_3_breach_or_denial_pauses_with_action_request() {
    // Budget breach
    {
        let fixture = FactoryFixture::new("budget-breach");
        let coord = CoordStore::new(fixture.workspace.join(".agent-workspace/coordination"));
        let idea_id = setup_ready_idea(&fixture, "Criterion 3 budget breach");
        let before_snap = snapshot::collect_with_factory(
            &fixture.workspace,
            &coord,
            Duration::from_secs(5),
            true,
        );
        let before_needs_you = before_snap.factory.as_ref().unwrap().needs_you;

        let run = create_run(
            &fixture.store,
            &fixture.workspace,
            &idea_id,
            CreateRunOptions {
                budget: Some(Budget {
                    max_cost_usd: Some(1.0),
                    max_duration_secs: Some(1),
                    ..Budget::default()
                }),
                ..CreateRunOptions::default()
            },
            &sys_actor(),
        )
        .unwrap();

        let final_state = drive_until_settled(&fixture, &run.id, 6);
        assert_eq!(final_state, RunState::Paused);

        let paused_run = get_run(&fixture.store, &run.id).unwrap().unwrap();
        assert!(!paused_run.action_commands.is_empty());
        assert!(paused_run.pause_reason.is_some());

        let log = read_all(&fixture.store.root.join("events.jsonl")).unwrap();
        let paused_event = log
            .events
            .iter()
            .rev()
            .find(|ev| ev.run.as_deref() == Some(&run.id) && ev.r#type == "run.paused")
            .expect("run.paused event must be recorded on budget breach");
        assert!(
            paused_event
                .data
                .get("cap")
                .and_then(|v| v.as_str())
                .is_some()
        );
        assert!(
            paused_event
                .data
                .get("spent")
                .and_then(|v| v.as_f64())
                .is_some()
        );
        assert!(
            paused_event
                .data
                .get("limit")
                .and_then(|v| v.as_f64())
                .is_some()
        );

        let after_snap = snapshot::collect_with_factory(
            &fixture.workspace,
            &coord,
            Duration::from_secs(5),
            true,
        );
        let after_needs_you = after_snap.factory.as_ref().unwrap().needs_you;
        assert!(
            after_needs_you > before_needs_you,
            "expected needs_you to increase ({before_needs_you} -> {after_needs_you})"
        );

        let stage_starts = log
            .events
            .iter()
            .filter(|ev| ev.run.as_deref() == Some(&run.id) && ev.r#type == "stage.started")
            .count();
        assert_eq!(
            stage_starts, 1,
            "no further stage should start after budget breach"
        );
    }

    // Policy denial
    {
        let fixture = FactoryFixture::new("happy");
        permissions::deny_target_tool(
            &fixture.store,
            "workspace",
            WorkflowTool::RepoWrite,
            "user",
            &user_caller(),
        )
        .unwrap();

        let idea_id = setup_ready_idea(&fixture, "Criterion 3 policy denial");
        let run = create_run(
            &fixture.store,
            &fixture.workspace,
            &idea_id,
            CreateRunOptions::default(),
            &sys_actor(),
        )
        .unwrap();

        let final_state = drive_until_settled(&fixture, &run.id, 6);
        assert_eq!(final_state, RunState::Paused);

        let paused_run = get_run(&fixture.store, &run.id).unwrap().unwrap();
        let reason = paused_run.pause_reason.as_deref().unwrap_or_default();
        assert!(
            reason.contains("repo_write"),
            "pause_reason should name denied tool: {reason}"
        );
        assert!(
            reason.contains("test-engineer"),
            "pause_reason should name agent: {reason}"
        );
        assert!(
            reason.contains("create-tests"),
            "pause_reason should name stage: {reason}"
        );
        assert!(
            !paused_run.action_commands.is_empty(),
            "policy denial must include action commands"
        );

        let log = read_all(&fixture.store.root.join("events.jsonl")).unwrap();
        let stage_starts: Vec<String> = log
            .events
            .iter()
            .filter(|ev| ev.run.as_deref() == Some(&run.id) && ev.r#type == "stage.started")
            .filter_map(|ev| {
                ev.data
                    .get("stage")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .collect();
        assert_eq!(
            stage_starts,
            vec!["plan".to_string()],
            "no further stage should start after policy denial at create-tests"
        );
    }
}

/// Criterion 4: "The resulting pull request is linked to its originating idea and contains test and review evidence."
#[test]
fn criterion_4_pr_is_linked_and_carries_evidence() {
    let fixture = FactoryFixture::new("happy");
    let idea_id = setup_ready_idea(&fixture, "Criterion 4 PR evidence");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    let final_state = drive_until_settled(&fixture, &run.id, 16);
    let final_run = get_run(&fixture.store, &run.id).unwrap().unwrap();
    assert_eq!(
        final_state,
        RunState::Delivered,
        "pause_reason={:?} stages={:?}",
        final_run.pause_reason,
        final_run.stages
    );

    let gh_log = fs::read_to_string(&fixture.gh_log_path).unwrap();
    let pr_create_line = gh_log
        .lines()
        .find(|line| line.starts_with("pr create "))
        .expect("gh pr create must be recorded");
    assert!(
        pr_create_line.contains("pr-body.md"),
        "gh pr create must pass pr-body.md: {pr_create_line}"
    );

    let pr_body = fs::read_to_string(fixture.store.run_dir(&run.id).join("pr-body.md")).unwrap();
    assert!(
        pr_body.contains(&idea_id),
        "PR body must name idea id {idea_id}"
    );
    assert!(pr_body.contains("## Tests"));
    assert!(pr_body.contains("## Review"));
    assert!(
        !pr_body.contains("No test evidence recorded."),
        "PR body must contain real test results"
    );
    assert!(
        !pr_body.contains("No review findings recorded."),
        "PR body must contain real review results"
    );

    let final_idea = fixture.store.get_idea(&idea_id).unwrap().unwrap();
    assert_eq!(final_idea.prs.len(), 1);
    assert_eq!(final_idea.prs[0].number, 42);
    assert_eq!(
        final_idea.prs[0].url,
        "https://github.com/acme/widget/pull/42"
    );

    let final_run = get_run(&fixture.store, &run.id).unwrap().unwrap();
    let raw_wf = load_workflow_file(&fixture.store.run_dir(&run.id).join("workflow.yaml")).unwrap();
    let wf = validate(&raw_wf, &fixture.store.settings().unwrap()).unwrap();
    assert!(
        evidence::missing(&final_run, &wf).is_empty(),
        "evidence::missing must be empty for delivered run"
    );
}

/// Criterion 5: "A user can pause or cancel safely and can resume from the last valid stage where policy permits."
#[test]
fn criterion_5_pause_cancel_resume_from_last_valid_stage() {
    // Pause during develop, then resume -> plan and create-tests do not re-run, run finishes
    {
        let fixture = FactoryFixture::new("happy");
        let idea_id = setup_ready_idea(&fixture, "Criterion 5 pause and resume");
        let run = create_run(
            &fixture.store,
            &fixture.workspace,
            &idea_id,
            CreateRunOptions::default(),
            &sys_actor(),
        )
        .unwrap();

        fixture.drive_step(&run.id).unwrap();
        fixture.drive_step(&run.id).unwrap();

        fixture.set_scenario("hang");
        std::thread::scope(|s| {
            let handle = s.spawn(|| fixture.drive_step(&run.id));
            wait_for_stage_running(&fixture, &run.id, "develop");
            pause_run_by_user(&fixture.store, &run.id, true, &user_actor(), &user_caller())
                .unwrap();
            let _ = handle.join().unwrap();
        });

        let paused = get_run(&fixture.store, &run.id).unwrap().unwrap();
        assert_eq!(paused.state, RunState::Paused);

        fixture.set_scenario("happy");
        let resumed = fixture.resume_run(&run.id).unwrap();
        assert_eq!(resumed.state, RunState::Running);

        let final_state = drive_until_settled(&fixture, &run.id, 16);
        assert_eq!(final_state, RunState::Delivered);

        let log = read_all(&fixture.store.root.join("events.jsonl")).unwrap();
        let stage_start_count = |stage: &str| {
            log.events
                .iter()
                .filter(|ev| {
                    ev.run.as_deref() == Some(&run.id)
                        && ev.r#type == "stage.started"
                        && ev.data.get("stage").and_then(|v| v.as_str()) == Some(stage)
                })
                .count()
        };
        assert_eq!(stage_start_count("plan"), 1);
        assert_eq!(stage_start_count("create-tests"), 1);
    }

    // Cancel during develop -> job stops, idea returns to ReadyForDevelopment, worktree & branch kept
    {
        let fixture = FactoryFixture::new("happy");
        let idea_id = setup_ready_idea(&fixture, "Criterion 5 cancel");
        let run = create_run(
            &fixture.store,
            &fixture.workspace,
            &idea_id,
            CreateRunOptions::default(),
            &sys_actor(),
        )
        .unwrap();

        fixture.drive_step(&run.id).unwrap();
        fixture.drive_step(&run.id).unwrap();

        fixture.set_scenario("hang");
        let cancel_res = std::thread::scope(|s| {
            let handle = s.spawn(|| fixture.drive_step(&run.id));
            wait_for_stage_running(&fixture, &run.id, "develop");
            let res = cancel_run(
                &fixture.store,
                &run.id,
                &run.id,
                &user_actor(),
                &user_caller(),
            )
            .unwrap();
            let _ = handle.join().unwrap();
            res
        });

        assert!(!cancel_res.stopped_jobs.is_empty());
        assert_eq!(cancel_res.run.state, RunState::Cancelled);
        assert_eq!(cancel_res.idea.state, IdeaState::ReadyForDevelopment);

        let wt = cancel_res
            .run
            .worktree_path
            .as_deref()
            .expect("worktree_path should still be recorded");
        assert!(
            std::path::Path::new(wt).is_dir(),
            "worktree must still exist after cancel"
        );
        let branch = cancel_res
            .run
            .branch
            .as_deref()
            .expect("branch should still be recorded");
        let branch_check = StdCommand::new("git")
            .args(["rev-parse", "--verify", branch])
            .current_dir(&fixture.workspace)
            .output()
            .unwrap();
        assert!(
            branch_check.status.success(),
            "branch {branch} must still exist after cancel"
        );
    }

    // Resume when signed out is refused
    {
        let fixture = FactoryFixture::new("happy");
        let idea_id = setup_ready_idea(&fixture, "Criterion 5 signed out gate");
        let run = create_run(
            &fixture.store,
            &fixture.workspace,
            &idea_id,
            CreateRunOptions::default(),
            &sys_actor(),
        )
        .unwrap();

        fixture.drive_step(&run.id).unwrap();
        pause_run_by_user(
            &fixture.store,
            &run.id,
            false,
            &user_actor(),
            &user_caller(),
        )
        .unwrap();

        let err = resume_paused_run_with_probe(
            &fixture.store,
            &run.id,
            &user_actor(),
            &fixture.signed_out_login_probe(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("Cannot resume"),
            "expected gate refusal on resume, got: {err}"
        );
    }
}
