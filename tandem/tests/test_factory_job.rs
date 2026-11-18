use std::fs;
use std::path::PathBuf;

use tandem::factory::broker::context::{
    ContextError, ContextInputs, MAX_CONTEXT_BYTES, assemble_context,
};
use tandem::factory::broker::job::{
    JobError, JobSpec, JobState, correlation_id, count_interrupted_attempts, prepare_job,
    read_job_state, recover_interrupted_job, run_job_idempotent, set_job_state,
};
use tandem::factory::broker::result::{JobResult, JobResultStatus, parse_and_validate_result};
use tandem::factory::store::FactoryStore;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-job-{}-{}-{}",
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

#[test]
fn correlation_id_and_context_manifest_with_512kib_cap() {
    assert_eq!(
        correlation_id("r-4h8k2m1x", "develop", 2),
        "r-4h8k2m1x.develop.2"
    );

    let ws = temp_ws("ctx");
    let snap = ws.join("plan-snapshot.md");
    let playbook = ws.join("playbook.md");
    let agent = ws.join("AGENT.md");
    fs::write(&snap, "# Plan\nGoal: CSV export\n").unwrap();
    fs::write(&playbook, "# Playbook\nRun TDD\n").unwrap();
    fs::write(&agent, "# Implementer\nWrite clean Rust\n").unwrap();

    let assembled = assemble_context(&ContextInputs {
        plan_snapshot_path: snap.clone(),
        acceptance_criteria: vec!["Valid CSV".into()],
        playbook_path: playbook.clone(),
        agent_def_path: agent.clone(),
        dependency_summaries: vec![("plan".into(), "Spec ready".into())],
        retry_rationale: Some("previous assertion failed".into()),
    })
    .unwrap();

    assert_eq!(assembled.manifest.items.len(), 3);
    assert!(assembled.manifest.total_bytes < MAX_CONTEXT_BYTES);
    assert!(assembled.prompt.contains("CSV export"));
    assert!(assembled.prompt.contains("Spec ready"));
    assert!(assembled.prompt.contains("previous assertion failed"));

    // Exceeding 512 KiB fails before starting
    fs::write(&snap, vec![b'x'; MAX_CONTEXT_BYTES + 1]).unwrap();
    let err = assemble_context(&ContextInputs {
        plan_snapshot_path: snap,
        acceptance_criteria: vec![],
        playbook_path: playbook,
        agent_def_path: agent,
        dependency_summaries: vec![],
        retry_rationale: None,
    })
    .unwrap_err();
    assert!(matches!(err, ContextError::CapExceeded { .. }));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn result_validation_fails_closed_on_missing_or_invalid_schema() {
    let ws = temp_ws("res");

    // Missing file -> failed with "no structured result"
    let missing = parse_and_validate_result(&ws.join("missing.json"), false);
    assert_eq!(missing.status, JobResultStatus::Failed);
    assert!(
        missing
            .rationale
            .as_deref()
            .unwrap()
            .contains("no structured result")
    );

    // Invalid JSON -> failed with "no structured result"
    let bad_json = ws.join("bad.json");
    fs::write(&bad_json, "{not json").unwrap();
    let parsed_bad = parse_and_validate_result(&bad_json, false);
    assert_eq!(parsed_bad.status, JobResultStatus::Failed);
    assert!(
        parsed_bad
            .rationale
            .as_deref()
            .unwrap()
            .contains("no structured result")
    );

    // Failed without rationale -> invalid
    let no_rat = ws.join("no_rat.json");
    fs::write(
        &no_rat,
        r#"{"status":"failed","summary":"oops","files_changed":[],"artifacts":[]}"#,
    )
    .unwrap();
    let parsed_no_rat = parse_and_validate_result(&no_rat, false);
    assert_eq!(parsed_no_rat.status, JobResultStatus::Failed);
    assert!(
        parsed_no_rat
            .rationale
            .as_deref()
            .unwrap()
            .contains("no structured result")
    );

    // Retry without hypothesis -> invalid
    let no_hyp = ws.join("no_hyp.json");
    fs::write(
        &no_hyp,
        r#"{"status":"succeeded","summary":"ok","files_changed":[],"artifacts":[]}"#,
    )
    .unwrap();
    let parsed_no_hyp = parse_and_validate_result(&no_hyp, true);
    assert_eq!(parsed_no_hyp.status, JobResultStatus::Failed);

    // Valid retry result
    let ok_retry = ws.join("ok_retry.json");
    fs::write(
        &ok_retry,
        r#"{"status":"succeeded","summary":"fixed","files_changed":["a.rs"],"artifacts":[],"hypothesis":"fix off-by-one"}"#,
    )
    .unwrap();
    let parsed_ok = parse_and_validate_result(&ok_retry, true);
    assert_eq!(parsed_ok.status, JobResultStatus::Succeeded);
    assert_eq!(parsed_ok.summary, "fixed");

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn prepare_job_is_idempotent_and_handles_crashed_interruptions() {
    let ws = temp_ws("job");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();

    let snap = ws.join("plan-snapshot.md");
    let playbook = ws.join("playbook.md");
    let agent = ws.join("AGENT.md");
    fs::write(&snap, "# Plan\n").unwrap();
    fs::write(&playbook, "# Playbook\n").unwrap();
    fs::write(&agent, "# Agent\n").unwrap();

    let ctx = assemble_context(&ContextInputs {
        plan_snapshot_path: snap,
        acceptance_criteria: vec![],
        playbook_path: playbook,
        agent_def_path: agent,
        dependency_summaries: vec![],
        retry_rationale: None,
    })
    .unwrap();

    let spec = JobSpec {
        run: "r-11112222".into(),
        stage: "develop".into(),
        attempt: 1,
        agent: "implementer".into(),
        provider: "claude".into(),
        model: "claude-sonnet-4-6".into(),
        tool_grants: vec!["read_file".into(), "edit_file".into()],
        cwd: ws
            .join("worktrees/factory-r-11112222")
            .to_string_lossy()
            .to_string(),
        time_limit_secs: 300,
    };

    let prep = prepare_job(&store, &spec, &ctx).unwrap();
    assert_eq!(read_job_state(&prep.dir).unwrap(), JobState::Prepared);
    assert!(prep.dir.join("input.json").exists());
    assert!(prep.dir.join("prompt.md").exists());

    // First run executes closure once
    let mut calls = 0usize;
    let res1 = run_job_idempotent(&store, &spec, &ctx, |_p| {
        calls += 1;
        Ok(JobResult {
            status: JobResultStatus::Succeeded,
            summary: "done".into(),
            files_changed: vec!["src/lib.rs".into()],
            artifacts: vec![],
            rationale: None,
            hypothesis: None,
            usage: None,
            no_behaviour_change: false,
        })
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(res1.status, JobResultStatus::Succeeded);
    assert_eq!(read_job_state(&prep.dir).unwrap(), JobState::Finished);

    // Second run with same correlation id returns stored result without calling runner
    let res2 = run_job_idempotent(&store, &spec, &ctx, |_p| {
        calls += 1;
        unreachable!("must not re-run finished job")
    })
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(res2, res1);

    // Crashed/interrupted handling: at most 3 interruptions per stage
    for attempt in 2..=5 {
        let s = JobSpec {
            attempt,
            ..spec.clone()
        };
        let p = prepare_job(&store, &s, &ctx).unwrap();
        set_job_state(&p.dir, JobState::Running).unwrap();
        if attempt <= 4 {
            recover_interrupted_job(&store, &s.run, &s.stage, attempt).unwrap();
            assert_eq!(read_job_state(&p.dir).unwrap(), JobState::Interrupted);
        } else {
            let err = recover_interrupted_job(&store, &s.run, &s.stage, attempt).unwrap_err();
            assert!(matches!(err, JobError::TooManyInterruptions { .. }));
        }
    }
    assert_eq!(
        count_interrupted_attempts(&store, &spec.run, &spec.stage).unwrap(),
        3
    );

    let _ = fs::remove_dir_all(&ws);
}
