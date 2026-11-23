mod common;

use std::fs;
use std::path::Path;
use std::process::Command as StdCommand;

use common::TempDir;
use common::factory_fixture::{orch_actor, sys_actor, user_actor};
use tandem::cli::{Cli, Command, parse_from};
use tandem::config::Config;
use tandem::factory::approval::Caller;
use tandem::factory::audit::{AuditFilter, entries as audit_entries};
use tandem::factory::commands::run_with_caller_io;
use tandem::factory::machine::{self, Command as MachineCommand, EventData, TransitionError};
use tandem::factory::model::{ActorKind, FeasibilityVerdict, Idea, TARGET_RATIONALE_MAX_BYTES};
use tandem::factory::store::FactoryStore;
use tandem::factory::workspace::{target_choices, validate_recommended_repo};

fn init_git_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    for args in [
        &["init", "-b", "trunk"][..],
        &["config", "user.email", "test@example.com"][..],
        &["config", "user.name", "Test"][..],
    ] {
        assert!(
            StdCommand::new("git")
                .args(args)
                .current_dir(dir)
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(dir.join("README.md"), "# Repo\n").unwrap();
    assert!(
        StdCommand::new("git")
            .args(["add", "README.md"])
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

fn link_repo(ws: &Path, repos_dir: &Path, name: &str) {
    let repo_path = repos_dir.join(name);
    init_git_repo(&repo_path);
    fs::create_dir_all(ws.join("codebase-symlinks")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&repo_path, ws.join("codebase-symlinks").join(name)).unwrap();
}

fn setup_idea_in_planning(ws: &Path) -> (FactoryStore, String) {
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let user = user_actor();
    let orch = orch_actor();
    let idea = store
        .create_idea("Export CSV", "Add CSV endpoint", "alice", &user)
        .unwrap();
    store
        .execute(&idea.id, MachineCommand::StartDiscovery, &orch)
        .unwrap();

    let feas = ws.join("feas.md");
    fs::write(&feas, "# Feasibility\nOk\n").unwrap();
    let feas_ref = tandem::factory::artifacts::put(&store, &idea.id, "feasibility", &feas).unwrap();
    store
        .execute(
            &idea.id,
            MachineCommand::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: Some(FeasibilityVerdict::Feasible),
                risk: None,
                workflow: None,
                missing_sections: vec![],
            },
            &orch,
        )
        .unwrap();
    store
        .execute(&idea.id, MachineCommand::SelectDirection, &orch)
        .unwrap();

    let plan = ws.join("plan.md");
    fs::write(
        &plan,
        "# Plan\n\n## Scope\nAPI\n\n## Acceptance criteria\n- Works\n\n## Risks\nLow\n\n## Estimates\n1d\n\n## Workflow recommendation\ndefault-delivery\n",
    )
    .unwrap();
    let plan_ref = tandem::factory::artifacts::put(&store, &idea.id, "plan", &plan).unwrap();
    store
        .execute(
            &idea.id,
            MachineCommand::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: vec![],
            },
            &orch,
        )
        .unwrap();
    (store, idea.id)
}

fn run_factory_cli(
    ws: &Path,
    state_dir: &Path,
    caller: &Caller<'_>,
    args: &[&str],
) -> (i32, String, String) {
    let mut full = vec!["tandem", "factory"];
    full.extend_from_slice(args);
    let Cli {
        command: Some(Command::Factory(factory_args)),
        ..
    } = parse_from(full).unwrap()
    else {
        panic!("expected factory command");
    };
    let cfg = Config {
        factory: true,
        state_dir: state_dir.to_path_buf(),
        ..Config::default()
    };
    let mut stdin = std::io::empty();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_caller_io(
        &factory_args,
        ws,
        &cfg,
        caller,
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    (
        code,
        String::from_utf8_lossy(&stdout).into_owned(),
        String::from_utf8_lossy(&stderr).into_owned(),
    )
}

#[test]
fn target_choices_and_validator_require_two_or_more_git_repos() {
    let ws = TempDir::new();
    let repos = TempDir::new();

    let empty = target_choices(ws.path());
    assert!(empty.repos.is_empty());
    assert!(!empty.required);
    assert!(validate_recommended_repo(ws.path(), "workspace").is_err());

    fs::create_dir_all(ws.path().join("codebase-symlinks/not-a-git-dir")).unwrap();
    link_repo(ws.path(), repos.path(), "api-service");
    let single = target_choices(ws.path());
    assert_eq!(single.repos, vec!["api-service"]);
    assert!(!single.required);
    let err_single = validate_recommended_repo(ws.path(), "api-service").unwrap_err();
    assert!(err_single.to_string().contains("only one target available"));

    link_repo(ws.path(), repos.path(), "web-client");
    let multi = target_choices(ws.path());
    assert_eq!(multi.repos, vec!["api-service", "web-client"]);
    assert!(multi.required);
    assert_eq!(
        validate_recommended_repo(ws.path(), "api-service").unwrap(),
        "api-service"
    );
    assert!(validate_recommended_repo(ws.path(), "workspace").is_err());
    assert!(validate_recommended_repo(ws.path(), "unknown-repo").is_err());
}

#[test]
fn machine_target_recommendation_rules_and_legacy_deserialization() {
    let ws = TempDir::new();
    let store = FactoryStore::new(ws.path());
    store.ensure().unwrap();
    let user = user_actor();
    let orch = orch_actor();
    let sys = sys_actor();

    let inbox = store.create_idea("Inbox", "Text", "alice", &user).unwrap();
    assert!(matches!(
        store.execute(
            &inbox.id,
            MachineCommand::RecommendTarget {
                repo: "api".into(),
                rationale: "because".into(),
            },
            &orch,
        ),
        Err(tandem::factory::FactoryError::Transition(
            TransitionError::IllegalTransition { .. }
        ))
    ));

    let (_plan_store, idea_id) = setup_idea_in_planning(ws.path());
    assert!(matches!(
        store.execute(
            &idea_id,
            MachineCommand::RecommendTarget {
                repo: "api".into(),
                rationale: "system actor".into(),
            },
            &sys,
        ),
        Err(tandem::factory::FactoryError::Transition(
            TransitionError::ActorNotAllowed { .. }
        ))
    ));

    assert!(matches!(
        store.execute(
            &idea_id,
            MachineCommand::RecommendTarget {
                repo: "api".into(),
                rationale: "   \n ".into(),
            },
            &orch,
        ),
        Err(tandem::factory::FactoryError::Transition(
            TransitionError::InvalidInput(_)
        ))
    ));

    let oversized = "x".repeat(TARGET_RATIONALE_MAX_BYTES + 1);
    assert!(matches!(
        store.execute(
            &idea_id,
            MachineCommand::RecommendTarget {
                repo: "api".into(),
                rationale: oversized,
            },
            &orch,
        ),
        Err(tandem::factory::FactoryError::Transition(
            TransitionError::InvalidInput(_)
        ))
    ));

    let updated = store
        .execute(
            &idea_id,
            MachineCommand::RecommendTarget {
                repo: "api-service".into(),
                rationale: "  Changes src/export.rs  ".into(),
            },
            &orch,
        )
        .unwrap();
    let rec = updated.target_recommendation.as_ref().unwrap();
    assert_eq!(rec.repo, "api-service");
    assert_eq!(rec.rationale, "Changes src/export.rs");
    assert_eq!(rec.by, orch);

    let replaced = store
        .execute(
            &idea_id,
            MachineCommand::RecommendTarget {
                repo: "web-client".into(),
                rationale: "Actually UI-only change".into(),
            },
            &user,
        )
        .unwrap();
    let rec2 = replaced.target_recommendation.as_ref().unwrap();
    assert_eq!(rec2.repo, "web-client");
    assert_eq!(rec2.by, user);

    assert!(!store.materialized_differs_from_log().unwrap());
    store.rebuild().unwrap();
    let rebuilt = store.get_idea(&idea_id).unwrap().unwrap();
    assert_eq!(
        rebuilt.target_recommendation,
        replaced.target_recommendation
    );

    store
        .execute(&idea_id, MachineCommand::MarkReady, &user)
        .unwrap();
    assert!(matches!(
        store.execute(
            &idea_id,
            MachineCommand::RecommendTarget {
                repo: "api-service".into(),
                rationale: "After gate 2".into(),
            },
            &orch,
        ),
        Err(tandem::factory::FactoryError::Transition(
            TransitionError::IllegalTransition { .. }
        ))
    ));

    let legacy_json = serde_json::json!({
        "id": "i-legacy01",
        "title": "Legacy",
        "text": "Old idea",
        "state": "planning",
        "owner": "alice",
        "created": 100.0,
        "updated": 100.0
    });
    let decoded: Idea = serde_json::from_value(legacy_json).unwrap();
    assert_eq!(decoded.target_recommendation, None);

    let ev = serde_json::to_value(&EventData::IdeaTargetRecommended {
        repo: "api-service".into(),
        rationale: "reason".into(),
    })
    .unwrap();
    assert_eq!(ev["type"], "idea.target_recommended");
    let _ = machine::apply(Some(decoded), &EventData::IdeaDiscoveryStarted {}, 101.0);
}

#[test]
fn cli_target_recommend_show_redaction_stale_and_audit() {
    let ws = TempDir::new();
    let state_dir = TempDir::new();
    let repos = TempDir::new();
    let (_store, idea_id) = setup_idea_in_planning(ws.path());

    let orch_env = |k: &str| {
        if k == "TANDEM_ROLE" {
            Some("orchestrator".to_string())
        } else if k == "TANDEM_SESSION_ID" {
            Some("orchestrator-1".to_string())
        } else {
            None
        }
    };
    let orch_caller = Caller::cli(true, &orch_env, &|_| None);

    let (code, _, err) = run_factory_cli(
        ws.path(),
        state_dir.path(),
        &orch_caller,
        &[
            "idea",
            "target-recommend",
            &idea_id,
            "--repo",
            "workspace",
            "--reason",
            "only one repo",
            "--json",
        ],
    );
    assert_eq!(code, 1);
    assert!(err.contains("only one target available"));

    link_repo(ws.path(), repos.path(), "api-service");
    link_repo(ws.path(), repos.path(), "web-client");

    let reason_file = ws.path().join("why.md");
    fs::write(
        &reason_file,
        "Modifies src/api.rs with token=ghp_1234567890abcdefghij1234567890abcdef",
    )
    .unwrap();

    let worker_env = |k: &str| (k == "TANDEM_ROLE").then(|| "implementer".to_string());
    let worker_caller = Caller::cli(true, &worker_env, &|_| None);
    let (w_code, _, _) = run_factory_cli(
        ws.path(),
        state_dir.path(),
        &worker_caller,
        &[
            "idea",
            "target-recommend",
            &idea_id,
            "--repo",
            "api-service",
            "--reason",
            "worker attempt",
            "--json",
        ],
    );
    assert_eq!(w_code, 3);

    let (ok_code, ok_out, ok_err) = run_factory_cli(
        ws.path(),
        state_dir.path(),
        &orch_caller,
        &[
            "idea",
            "target-recommend",
            &idea_id,
            "--repo",
            "api-service",
            "--reason-file",
            reason_file.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(ok_code, 0, "stderr: {ok_err}");
    let rec_json: serde_json::Value = serde_json::from_str(&ok_out).unwrap();
    assert_eq!(rec_json["idea"], idea_id);
    assert_eq!(rec_json["target_recommendation"]["repo"], "api-service");
    assert_eq!(
        rec_json["target_recommendation"]["by"]["kind"],
        serde_json::json!(ActorKind::Orchestrator)
    );
    assert!(
        !ok_out.contains("ghp_1234567890"),
        "secret token must be redacted: {ok_out}"
    );

    let (show_code, show_out, _) = run_factory_cli(
        ws.path(),
        state_dir.path(),
        &orch_caller,
        &["idea", "show", &idea_id, "--json"],
    );
    assert_eq!(show_code, 0);
    let show_json: serde_json::Value = serde_json::from_str(&show_out).unwrap();
    assert_eq!(show_json["target_choices"]["required"], true);
    assert_eq!(
        show_json["target_choices"]["repos"],
        serde_json::json!(["api-service", "web-client"])
    );
    assert_eq!(show_json["target_recommendation"]["repo"], "api-service");
    assert_eq!(show_json["target_recommendation"]["stale"], false);

    let entries = audit_entries(
        &FactoryStore::new(ws.path()),
        &idea_id,
        &AuditFilter::default(),
    )
    .unwrap();
    assert!(
        entries
            .iter()
            .any(|e| e.summary.contains("target recommended: api-service (by ")),
        "audit summaries: {:?}",
        entries.iter().map(|e| &e.summary).collect::<Vec<_>>()
    );

    fs::remove_file(ws.path().join("codebase-symlinks/api-service")).unwrap();
    let (stale_code, stale_out, _) = run_factory_cli(
        ws.path(),
        state_dir.path(),
        &orch_caller,
        &["idea", "show", &idea_id, "--json"],
    );
    assert_eq!(stale_code, 0);
    let stale_json: serde_json::Value = serde_json::from_str(&stale_out).unwrap();
    assert_eq!(stale_json["target_recommendation"]["stale"], true);
}
