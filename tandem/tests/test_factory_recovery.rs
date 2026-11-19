mod common;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use common::factory_fixture::{FactoryFixture, orch_actor, sys_actor, user_actor, user_caller};
use tandem::config::Config;
use tandem::factory::approval::Caller;
use tandem::factory::broker::git::{AllowedGitSubcommand, run_git};
use tandem::factory::broker::job::{JobState, job_dir, read_job_state, set_job_state};
use tandem::factory::broker::result::{JobResult, JobResultStatus, JobUsage};
use tandem::factory::commands::{
    FactoryArgs, FactoryCommand, FactoryDoctorArgs, run_with_caller_io,
};
use tandem::factory::events::{append, read_all};
use tandem::factory::machine::EventData;
use tandem::factory::model::{RunState, StageState};
use tandem::factory::recovery::{
    FindingSeverity, ProblemKind, diagnose, doctor_fix, run_safe_repairs, run_startup_repairs,
};
use tandem::factory::run::{CreateRunOptions, create_run, get_run};
use tandem::factory::runner::drive_run_step_with_gh;
use tandem::factory::workspace::prepare_worktree;

fn make_config(workspace: &Path) -> Config {
    Config {
        workspace: Some(workspace.to_path_buf()),
        state_dir: workspace.join(".agent-workspace/state"),
        factory: true,
        ..Default::default()
    }
}

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
            tandem::factory::machine::Command::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some("conv-rec-1".to_string()),
            },
            &user,
        )
        .unwrap();
    fixture
        .store
        .execute(
            &idea.id,
            tandem::factory::machine::Command::Describe {
                title: title.to_string(),
                text: "Export team activity as CSV filtered by --from and --to dates.".to_string(),
            },
            &orch,
        )
        .unwrap();
    let feas_src = fixture.workspace.join(".agent-workspace/feas.md");
    fs::write(&feas_src, "# Feasibility\n\nFeasible.\n").unwrap();
    let feas_ref =
        tandem::factory::artifacts::put(&fixture.store, &idea.id, "feasibility", &feas_src)
            .unwrap();
    fixture
        .store
        .execute(
            &idea.id,
            tandem::factory::machine::Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: Some(tandem::factory::model::FeasibilityVerdict::Feasible),
                risk: Some(tandem::factory::model::Risk::Low),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();
    fixture
        .store
        .execute(
            &idea.id,
            tandem::factory::machine::Command::SelectDirection,
            &user,
        )
        .unwrap();
    let plan_src = fixture.workspace.join(".agent-workspace/plan.md");
    let plan_body = "# Plan\n\n## Scope\nAdd `src/export.sh` to export team activity as CSV with `--from` and `--to` date filters.\n\n## Acceptance criteria\n- Export team activity to CSV\n- Filter exported activity by date range\n\n## Risks\nLow risk; isolated shell script.\n\n## Estimates\n1 day.\n\n## Workflow recommendation\ndefault-delivery\n";
    fs::write(&plan_src, plan_body).unwrap();
    let plan_ref =
        tandem::factory::artifacts::put(&fixture.store, &idea.id, "plan", &plan_src).unwrap();
    let missing = tandem::factory::artifacts::missing_plan_sections(plan_body);
    fixture
        .store
        .execute(
            &idea.id,
            tandem::factory::machine::Command::StoreArtifact {
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
    tandem::factory::approval::mark_ready(
        &fixture.store,
        &idea.id,
        &idea.id,
        "user",
        &user_caller(),
    )
    .unwrap();
    idea.id
}

fn drive_until_settled(fixture: &FactoryFixture, run_id: &str, max_steps: usize) -> RunState {
    for _ in 0..max_steps {
        fixture.drive_step(run_id).unwrap();
        let run = get_run(&fixture.store, run_id).unwrap().unwrap();
        if matches!(
            run.state,
            RunState::Delivered | RunState::Paused | RunState::Failed | RunState::Cancelled
        ) {
            return run.state;
        }
    }
    get_run(&fixture.store, run_id).unwrap().unwrap().state
}

#[test]
fn doctor_detects_and_repairs_stale_runner_lock() {
    let fixture = FactoryFixture::new("happy");
    let lock_path = fixture.store.root.join(".runner.lock");
    fs::write(&lock_path, "999999\n").unwrap();

    let report = diagnose(&fixture.store, &fixture.workspace).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == ProblemKind::StaleRunnerLock && !f.repaired)
    );

    let repaired = run_safe_repairs(&fixture.store, &fixture.workspace).unwrap();
    assert!(
        repaired
            .findings
            .iter()
            .any(|f| f.kind == ProblemKind::StaleRunnerLock
                && f.repaired
                && f.severity == FindingSeverity::Fixed)
    );
    assert!(!lock_path.exists());
}

#[test]
fn doctor_detects_and_repairs_running_job_with_stale_lock() {
    let fixture = FactoryFixture::new("happy");
    let idea_id = setup_ready_idea(&fixture, "Recover running job");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    let sys = sys_actor();
    let evs = vec![
        tandem::factory::events::redact_event(
            tandem::factory::events::Event::from_data(
                1.0,
                sys.clone(),
                idea_id.clone(),
                Some(run.id.clone()),
                &EventData::RunStarted {
                    run: run.id.clone(),
                },
            )
            .unwrap(),
            &tandem::factory::redact::RedactContext::from_env(),
        ),
        tandem::factory::events::redact_event(
            tandem::factory::events::Event::from_data(
                2.0,
                sys,
                idea_id.clone(),
                Some(run.id.clone()),
                &EventData::StageStarted {
                    run: run.id.clone(),
                    stage: "plan".to_string(),
                    attempt: 1,
                    grant: None,
                },
            )
            .unwrap(),
            &tandem::factory::redact::RedactContext::from_env(),
        ),
    ];
    append(
        &fixture.store.root.join("events.jsonl"),
        &fixture.store.root.join(".lock"),
        &evs,
    )
    .unwrap();
    fixture.store.rebuild().unwrap();

    let jdir = job_dir(&fixture.store, &run.id, "plan", 1);
    fs::create_dir_all(&jdir).unwrap();
    set_job_state(&jdir, JobState::Running).unwrap();
    fs::write(fixture.store.root.join(".runner.lock"), "999999\n").unwrap();

    let diag = diagnose(&fixture.store, &fixture.workspace).unwrap();
    assert!(
        diag.findings
            .iter()
            .any(|f| f.kind == ProblemKind::InterruptedRunningJob)
    );

    let fixed = run_safe_repairs(&fixture.store, &fixture.workspace).unwrap();
    assert!(
        fixed
            .findings
            .iter()
            .any(|f| f.kind == ProblemKind::InterruptedRunningJob && f.repaired)
    );
    assert_eq!(read_job_state(&jdir).unwrap(), JobState::Interrupted);

    let updated_run = get_run(&fixture.store, &run.id).unwrap().unwrap();
    let plan_stage = updated_run
        .stages
        .iter()
        .find(|s| s.name == "plan")
        .unwrap();
    assert_eq!(plan_stage.state, StageState::Pending);
    assert_eq!(plan_stage.interrupted_attempts, 1);

    let final_state = drive_until_settled(&fixture, &run.id, 12);
    let r = get_run(&fixture.store, &run.id).unwrap().unwrap();
    assert_eq!(
        final_state,
        RunState::Delivered,
        "pause_reason: {:?}",
        r.pause_reason
    );
}

#[test]
fn doctor_repairs_half_created_run_and_marks_missing_event_failed() {
    let fixture = FactoryFixture::new("happy");
    let idea_id = setup_ready_idea(&fixture, "Half created run");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    // Case 1: run.json deleted, RunCreated event exists -> rebuilt from log.
    let run_json = fixture.store.run_dir(&run.id).join("run.json");
    fs::remove_file(&run_json).unwrap();

    let diag = diagnose(&fixture.store, &fixture.workspace).unwrap();
    assert!(
        diag.findings
            .iter()
            .any(|f| f.kind == ProblemKind::HalfCreatedRun)
    );

    run_safe_repairs(&fixture.store, &fixture.workspace).unwrap();
    assert!(run_json.is_file());
    let rebuilt = get_run(&fixture.store, &run.id).unwrap().unwrap();
    assert_eq!(rebuilt.state, RunState::Queued);

    // Case 2: orphan run directory with no RunCreated event in log -> marked failed with reason.
    let ghost_id = "r-ghost001";
    let ghost_dir = fixture.store.run_dir(ghost_id);
    fs::create_dir_all(&ghost_dir).unwrap();
    run_safe_repairs(&fixture.store, &fixture.workspace).unwrap();
    let ghost = get_run(&fixture.store, ghost_id).unwrap().unwrap();
    assert_eq!(ghost.state, RunState::Failed);
    assert!(ghost.pause_reason.is_some());
}

#[test]
fn doctor_rebuilds_deleted_materialized_files_from_log() {
    let fixture = FactoryFixture::new("happy");
    let idea_id = setup_ready_idea(&fixture, "Rebuild materialized");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    fs::remove_file(
        fixture
            .store
            .root
            .join("ideas")
            .join(format!("{idea_id}.json")),
    )
    .unwrap();
    fs::remove_file(fixture.store.run_dir(&run.id).join("run.json")).unwrap();

    let diag = diagnose(&fixture.store, &fixture.workspace).unwrap();
    assert!(
        diag.findings
            .iter()
            .any(|f| f.kind == ProblemKind::MaterializedDrift)
    );

    let repaired = run_safe_repairs(&fixture.store, &fixture.workspace).unwrap();
    assert!(
        repaired
            .findings
            .iter()
            .any(|f| f.kind == ProblemKind::MaterializedDrift && f.repaired)
    );
    assert!(fixture.store.get_idea(&idea_id).unwrap().is_some());
    assert!(get_run(&fixture.store, &run.id).unwrap().is_some());
}

#[test]
fn doctor_reports_truncated_last_log_line_and_appends_continue() {
    let fixture = FactoryFixture::new("happy");
    let idea_id = setup_ready_idea(&fixture, "Truncated log line");

    let events_path = fixture.store.root.join("events.jsonl");
    let mut f = OpenOptions::new().append(true).open(&events_path).unwrap();
    f.write_all(b"{\"v\":1,\"seq\":999,\"truncated").unwrap();
    drop(f);

    let diag = diagnose(&fixture.store, &fixture.workspace).unwrap();
    assert!(diag.truncated_log);
    assert!(
        diag.findings
            .iter()
            .any(|f| f.kind == ProblemKind::TruncatedLogLine && !f.repaired)
    );

    // Appending a new event continues cleanly from the last complete line.
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();
    let log = read_all(&events_path).unwrap();
    assert!(!log.truncated);
    assert!(get_run(&fixture.store, &run.id).unwrap().is_some());
}

#[test]
fn doctor_reports_hash_chain_break_without_modifying_log() {
    let fixture = FactoryFixture::new("happy");
    let _idea_id = setup_ready_idea(&fixture, "Broken hash chain");
    let events_path = fixture.store.root.join("events.jsonl");
    let original = fs::read_to_string(&events_path).unwrap();
    let mut lines: Vec<String> = original.lines().map(ToString::to_string).collect();
    assert!(lines.len() >= 2);
    lines[1] = lines[1].replace("\"prev\":\"", "\"prev\":\"0000");
    let tampered = format!("{}\n", lines.join("\n"));
    fs::write(&events_path, &tampered).unwrap();

    let diag = diagnose(&fixture.store, &fixture.workspace).unwrap();
    assert!(!diag.chain_intact);
    assert!(
        diag.findings
            .iter()
            .any(|f| f.kind == ProblemKind::HashChainBreak && !f.repaired)
    );

    run_safe_repairs(&fixture.store, &fixture.workspace).unwrap();
    assert_eq!(fs::read_to_string(&events_path).unwrap(), tampered);
}

#[test]
fn doctor_fix_is_user_only_and_handles_clean_vs_dirty_orphan_worktrees() {
    let fixture = FactoryFixture::new("happy");

    // Create two runs with worktrees, then cancel both so their worktrees & branches become orphans.
    let idea_clean = setup_ready_idea(&fixture, "Clean orphan");
    let run_clean = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_clean,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();
    let ws_clean = prepare_worktree(&fixture.store, &fixture.workspace, &run_clean.id).unwrap();
    tandem::factory::runner::cancel_run(
        &fixture.store,
        &run_clean.id,
        &run_clean.id,
        &user_actor(),
        &user_caller(),
    )
    .unwrap();

    let idea_dirty = setup_ready_idea(&fixture, "Dirty orphan");
    let run_dirty = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_dirty,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();
    let ws_dirty = prepare_worktree(&fixture.store, &fixture.workspace, &run_dirty.id).unwrap();
    fs::write(ws_dirty.worktree_path.join("uncommitted.txt"), "wip\n").unwrap();
    tandem::factory::runner::cancel_run(
        &fixture.store,
        &run_dirty.id,
        &run_dirty.id,
        &user_actor(),
        &user_caller(),
    )
    .unwrap();

    // Agent caller must be rejected from `doctor --fix`.
    let agent_caller = Caller::cli(
        false,
        &|k| (k == "TANDEM_ROLE").then(|| "agent".to_string()),
        &|_| None,
    );
    let args_fix = FactoryArgs {
        command: FactoryCommand::Doctor(FactoryDoctorArgs {
            fix: true,
            json: true,
        }),
    };
    let cfg = make_config(&fixture.workspace);
    let mut empty_in = std::io::Cursor::new(Vec::<u8>::new());
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_caller_io(
        &args_fix,
        &fixture.workspace,
        &cfg,
        &agent_caller,
        &mut empty_in,
        &mut out,
        &mut err,
    );
    assert_ne!(
        code, 0,
        "agent caller must not be allowed to run doctor --fix"
    );
    assert!(ws_clean.worktree_path.exists());
    assert!(ws_dirty.worktree_path.exists());

    // Wrong typed confirmation keeps clean worktree.
    let mut wrong_in = std::io::Cursor::new(b"wrong-id\n".to_vec());
    let mut prompt_out = Vec::new();
    let report_wrong = doctor_fix(
        &fixture.store,
        &fixture.workspace,
        &user_caller(),
        &mut wrong_in,
        &mut prompt_out,
    )
    .unwrap();
    assert_eq!(report_wrong.worktrees_removed, 0);
    assert!(ws_clean.worktree_path.exists());

    // Correct typed confirmation removes clean orphan worktree, keeps dirty worktree, and never deletes branches.
    let input_str = format!("{}\n", run_clean.id);
    let mut right_in = std::io::Cursor::new(input_str.into_bytes());
    let mut prompt_out2 = Vec::new();
    let report_ok = doctor_fix(
        &fixture.store,
        &fixture.workspace,
        &user_caller(),
        &mut right_in,
        &mut prompt_out2,
    )
    .unwrap();
    assert_eq!(report_ok.worktrees_removed, 1);
    assert!(!ws_clean.worktree_path.exists());
    assert!(
        ws_dirty.worktree_path.exists(),
        "dirty worktree must never be deleted"
    );

    // Branches must still exist.
    let branches = run_git(
        AllowedGitSubcommand::Branch,
        &["--format=%(refname:short)"],
        &fixture.workspace,
        std::time::Duration::from_secs(5),
    )
    .unwrap();
    assert!(branches.lines().any(|b| b.trim() == ws_clean.branch));
    assert!(branches.lines().any(|b| b.trim() == ws_dirty.branch));
    assert!(report_ok.orphan_branches.len() >= 2);
}

#[test]
fn crash_mid_stage_and_boundaries_resume_to_single_pr() {
    // 1. crash-mid-stage scenario -> paused/interrupted -> switch to happy -> resume -> single PR.
    let fixture = FactoryFixture::new("crash-mid-stage");
    let idea_id = setup_ready_idea(&fixture, "Crash mid stage");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    let state = drive_until_settled(&fixture, &run.id, 6);
    assert_eq!(state, RunState::Paused);

    // 2. Runner killed mid-stage (e.g. hang/SIGKILL) with stale lock -> safe repair & resume -> single PR.
    let fixture2 = FactoryFixture::new("happy");
    let idea2 = setup_ready_idea(&fixture2, "Crash mid stage runner kill");
    let run2 = create_run(
        &fixture2.store,
        &fixture2.workspace,
        &idea2,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();
    // Drive plan to completion, then simulate runner dying mid-stage during `create-tests`.
    fixture2.drive_step(&run2.id).unwrap();
    let sys = sys_actor();
    let ev = tandem::factory::events::redact_event(
        tandem::factory::events::Event::from_data(
            10.0,
            sys,
            idea2,
            Some(run2.id.clone()),
            &EventData::StageStarted {
                run: run2.id.clone(),
                stage: "create-tests".to_string(),
                attempt: 1,
                grant: None,
            },
        )
        .unwrap(),
        &tandem::factory::redact::RedactContext::from_env(),
    );
    append(
        &fixture2.store.root.join("events.jsonl"),
        &fixture2.store.root.join(".lock"),
        &[ev],
    )
    .unwrap();
    fixture2.store.rebuild().unwrap();
    let jdir = job_dir(&fixture2.store, &run2.id, "create-tests", 1);
    fs::create_dir_all(&jdir).unwrap();
    set_job_state(&jdir, JobState::Running).unwrap();
    fs::write(fixture2.store.root.join(".runner.lock"), "999999\n").unwrap();

    run_safe_repairs(&fixture2.store, &fixture2.workspace).unwrap();
    let final_state = drive_until_settled(&fixture2, &run2.id, 12);
    let delivered = get_run(&fixture2.store, &run2.id).unwrap().unwrap();
    assert_eq!(
        final_state,
        RunState::Delivered,
        "pause_reason: {:?}",
        delivered.pause_reason
    );
    assert_eq!(delivered.prs.len(), 1);
}

#[test]
fn crash_between_job_end_and_event_reuses_result_json() {
    let fixture = FactoryFixture::new("happy");
    let idea_id = setup_ready_idea(&fixture, "Crash after result json");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();
    let ws = prepare_worktree(&fixture.store, &fixture.workspace, &run.id).unwrap();

    // Emit RunStarted + StageStarted for plan attempt 1, and write result.json as if job finished right before crash.
    let sys = sys_actor();
    let evs = vec![
        tandem::factory::events::redact_event(
            tandem::factory::events::Event::from_data(
                1.0,
                sys.clone(),
                idea_id.clone(),
                Some(run.id.clone()),
                &EventData::RunStarted {
                    run: run.id.clone(),
                },
            )
            .unwrap(),
            &tandem::factory::redact::RedactContext::from_env(),
        ),
        tandem::factory::events::redact_event(
            tandem::factory::events::Event::from_data(
                2.0,
                sys,
                idea_id.clone(),
                Some(run.id.clone()),
                &EventData::StageStarted {
                    run: run.id.clone(),
                    stage: "plan".to_string(),
                    attempt: 1,
                    grant: None,
                },
            )
            .unwrap(),
            &tandem::factory::redact::RedactContext::from_env(),
        ),
    ];
    append(
        &fixture.store.root.join("events.jsonl"),
        &fixture.store.root.join(".lock"),
        &evs,
    )
    .unwrap();
    fixture.store.rebuild().unwrap();

    let jdir = job_dir(&fixture.store, &run.id, "plan", 1);
    fs::create_dir_all(&jdir).unwrap();
    set_job_state(&jdir, JobState::Running).unwrap();
    let result = JobResult {
        status: JobResultStatus::Succeeded,
        summary: "Plan completed before crash".to_string(),
        rationale: None,
        hypothesis: None,
        files_changed: Vec::new(),
        artifacts: vec!["slices.json".to_string(), "specs/t-001.md".to_string()],
        no_behaviour_change: false,
        usage: Some(JobUsage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            cost_usd: Some(0.01),
        }),
    };
    let artifact_dir = fixture.store.run_dir(&run.id).join("artifacts");
    fs::create_dir_all(artifact_dir.join("specs")).unwrap();
    fs::write(
        artifact_dir.join("specs/t-001.md"),
        "# t-001: Team activity export\n\n## Scope\nExport team activity as CSV with date range filtering.\n",
    )
    .unwrap();
    fs::write(
        artifact_dir.join("slices.json"),
        r#"{
  "phase": "p-001-activity-export",
  "slices": [
    {
      "id": "s-001",
      "title": "Team activity CSV export",
      "type": "New Feature",
      "paths": ["src/export.sh", "test/export_test.sh"],
      "dependencies": [],
      "acceptance_criteria": [
        "Export team activity to CSV",
        "Filter exported activity by date range"
      ],
      "ui": false
    }
  ]
}"#,
    )
    .unwrap();
    fs::write(
        jdir.join("result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();

    // Switch scenario to `hang` to prove `plan` is NOT re-executed (it reuses result.json)!
    fixture.set_scenario("hang");
    run_safe_repairs(&fixture.store, &fixture.workspace).unwrap();
    assert_eq!(read_job_state(&jdir).unwrap(), JobState::Finished);

    let mut exec = fixture.executor();
    drive_run_step_with_gh(
        &fixture.store,
        &fixture.workspace,
        &run.id,
        &mut exec,
        &fixture.gh_bin,
    )
    .unwrap();

    let after = get_run(&fixture.store, &run.id).unwrap().unwrap();
    let plan_stage = after.stages.iter().find(|s| s.name == "plan").unwrap();
    assert_eq!(
        plan_stage.state,
        StageState::Succeeded,
        "rationale: {:?}",
        plan_stage.rationale
    );
    assert_eq!(plan_stage.attempts, 1, "plan must not be re-run");
    let _ = ws;
}

#[test]
fn crash_between_push_and_pr_create_produces_single_pr() {
    let fixture = FactoryFixture::new("happy");
    let idea_id = setup_ready_idea(&fixture, "Crash after push");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    let final_state = drive_until_settled(&fixture, &run.id, 12);
    let r = get_run(&fixture.store, &run.id).unwrap().unwrap();
    assert_eq!(
        final_state,
        RunState::Delivered,
        "pause_reason: {:?}",
        r.pause_reason
    );
    let delivered = get_run(&fixture.store, &run.id).unwrap().unwrap();
    assert_eq!(delivered.prs.len(), 1);
    let gh_log = fs::read_to_string(&fixture.gh_log_path).unwrap();
    let creates = gh_log.lines().filter(|l| l.contains("pr create")).count();
    assert_eq!(creates, 1);
}

#[test]
fn startup_repairs_return_one_line_per_repair() {
    let fixture = FactoryFixture::new("happy");
    fs::write(fixture.store.root.join(".runner.lock"), "999999\n").unwrap();
    let lines = run_startup_repairs(&fixture.store, &fixture.workspace).unwrap();
    assert!(!lines.is_empty());
    assert!(lines.iter().all(|l| l.starts_with("fixed ")));
}
