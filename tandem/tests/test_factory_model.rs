use tandem::factory::ids::{is_valid_idea_id, is_valid_run_id, new_idea_id, new_run_id};
use tandem::factory::model::{
    Actor, ActorKind, Approval, ApprovalGate, ArtifactRef, Budget, FactorySettings, Idea,
    IdeaState, OrchestratorRef, Plan, PrLink, Risk, Run, RunState, Stage, StageState,
    validate_owner, validate_text, validate_title,
};

#[test]
fn test_idea_and_run_ids_generate_valid_format() {
    for _ in 0..32 {
        let idea_id = new_idea_id();
        assert!(is_valid_idea_id(&idea_id), "invalid idea id: {idea_id}");
        assert!(!is_valid_run_id(&idea_id));

        let run_id = new_run_id();
        assert!(is_valid_run_id(&run_id), "invalid run id: {run_id}");
        assert!(!is_valid_idea_id(&run_id));
    }
}

#[test]
fn test_id_validators_reject_invalid_inputs() {
    assert!(is_valid_idea_id("i-a1b2c3d4"));
    assert!(!is_valid_idea_id("r-a1b2c3d4"));
    assert!(!is_valid_idea_id("i-a1b2c3d"));
    assert!(!is_valid_idea_id("i-a1b2c3d45"));
    assert!(!is_valid_idea_id("i-A1b2c3d4"));
    assert!(!is_valid_idea_id("i-a1b2_3d4"));
    assert!(!is_valid_idea_id(""));

    assert!(is_valid_run_id("r-0123abcd"));
    assert!(!is_valid_run_id("i-0123abcd"));
    assert!(!is_valid_run_id("r-0123abc"));
    assert!(!is_valid_run_id("r-0123abcde"));
    assert!(!is_valid_run_id("r-0123ABCd"));
    assert!(!is_valid_run_id("r-0123-bcd"));
    assert!(!is_valid_run_id(""));
}

#[test]
fn test_idea_state_labels_terminal_and_snake_case() {
    let cases = [
        (IdeaState::Inbox, "\"inbox\"", "Inbox", false),
        (IdeaState::Ideation, "\"ideation\"", "Ideation", false),
        (IdeaState::Planning, "\"planning\"", "Planning", false),
        (
            IdeaState::ReadyForDevelopment,
            "\"ready_for_development\"",
            "Ready for development",
            false,
        ),
        (IdeaState::Queued, "\"queued\"", "Queued", false),
        (
            IdeaState::InDevelopment,
            "\"in_development\"",
            "In development",
            false,
        ),
        (IdeaState::Review, "\"review\"", "Review", false),
        (IdeaState::Delivered, "\"delivered\"", "Delivered", true),
        (IdeaState::Cancelled, "\"cancelled\"", "Cancelled", true),
    ];

    for (state, json, label, terminal) in cases {
        assert_eq!(serde_json::to_string(&state).unwrap(), json);
        assert_eq!(serde_json::from_str::<IdeaState>(json).unwrap(), state);
        assert_eq!(state.label(), label);
        assert_eq!(state.is_terminal(), terminal);
    }
}

#[test]
fn test_all_model_types_round_trip_json() {
    let actor = Actor {
        kind: ActorKind::User,
        id: "tui".to_string(),
    };
    let artifact = ArtifactRef {
        kind: "plan".to_string(),
        version: 1,
        path: "artifacts/i-12345678/plan-v1.md".to_string(),
        sha256: "deadbeef".to_string(),
        bytes: 512,
        created: 1700000000.0,
    };
    let plan = Plan {
        artifact: artifact.clone(),
        workflow: "default".to_string(),
        acceptance_criteria: vec!["Tests pass".to_string()],
        missing_sections: Vec::new(),
    };
    let approval = Approval {
        gate: ApprovalGate::ReadyForDevelopment,
        actor: actor.clone(),
        via: "tui".to_string(),
        at: 1700000010.0,
        subject: Some("i-12345678".to_string()),
    };
    let budget = Budget {
        max_cost_usd: Some(5.0),
        max_duration_secs: Some(600),
        max_retries: Some(2),
    };
    let stage = Stage {
        name: "implement".to_string(),
        agent: "senior-backend-dev".to_string(),
        state: StageState::Succeeded,
        attempts: 1,
        interrupted_attempts: 0,
        started: Some(1700000020.0),
        finished: Some(1700000080.0),
        artifacts: vec![artifact.clone()],
        rationale: Some("clean".to_string()),
        retry_hypothesis: None,
    };
    let run = Run {
        id: "r-12345678".to_string(),
        idea: "i-12345678".to_string(),
        workflow: "default".to_string(),
        workflow_version: "1".to_string(),
        workflow_sha256: "cafebabe".to_string(),
        workflow_source: "embedded".to_string(),
        plan_sha256: "deadbeef".to_string(),
        deploy_requested: false,
        target: None,
        branch: None,
        base_commit: None,
        worktree_path: None,
        pause_reason: None,
        action_commands: Vec::new(),
        baseline_passed: None,
        state: RunState::Running,
        stages: vec![stage.clone()],
        evidence: Vec::new(),
        prs: Vec::new(),
        budget: Some(budget.clone()),
        cost: None,
        created: 1700000015.0,
        updated: 1700000080.0,
    };
    let pr = PrLink {
        repo: "agent-workspace-package".to_string(),
        number: 42,
        url: "https://example.invalid/pr/42".to_string(),
        state: None,
        checks_summary: None,
    };
    let idea = Idea {
        id: "i-12345678".to_string(),
        title: "Add export".to_string(),
        text: "Export data to CSV".to_string(),
        state: IdeaState::Planning,
        owner: "alice".to_string(),
        created: 1700000000.0,
        updated: 1700000010.0,
        risk: Some(Risk::Medium),
        orchestrator: Some(OrchestratorRef {
            cli: "claude".to_string(),
            model: "sonnet".to_string(),
            conversation: Some("conv-1".to_string()),
        }),
        feasibility: Some(artifact.clone()),
        feasibility_verdict: None,
        plan: Some(plan.clone()),
        plan_snapshot: Some(artifact.clone()),
        ready_via: None,
        ready_at: None,
        artifacts: Vec::new(),
        assumptions: vec!["Single repo".to_string()],
        runs: vec![run.id.clone()],
        prs: vec![pr.clone()],
        cancelled_from: None,
    };
    let settings = FactorySettings {
        max_parallel_agents: 2,
        default_temperature: 0.2,
        default_budget: budget.clone(),
        ..FactorySettings::default()
    };

    macro_rules! assert_round_trip {
        ($val:expr, $ty:ty) => {
            let json = serde_json::to_string(&$val).unwrap();
            let decoded: $ty = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, $val);
        };
    }

    assert_round_trip!(Risk::Low, Risk);
    assert_round_trip!(actor, Actor);
    assert_round_trip!(artifact, ArtifactRef);
    assert_round_trip!(plan, Plan);
    assert_round_trip!(approval, Approval);
    assert_round_trip!(budget, Budget);
    assert_round_trip!(stage, Stage);
    assert_round_trip!(run, Run);
    assert_round_trip!(pr, PrLink);
    assert_round_trip!(idea, Idea);
    assert_round_trip!(settings, FactorySettings);
}

#[test]
fn test_older_records_missing_optional_fields_still_load() {
    let minimal_idea = r#"{
        "id": "i-abcd1234",
        "title": "Minimal",
        "state": "inbox",
        "owner": "bob",
        "created": 1.0,
        "updated": 1.0
    }"#;
    let idea: Idea = serde_json::from_str(minimal_idea).unwrap();
    assert_eq!(idea.text, "");
    assert_eq!(idea.risk, None);
    assert_eq!(idea.orchestrator, None);
    assert!(idea.assumptions.is_empty());
    assert!(idea.runs.is_empty());
    assert!(idea.prs.is_empty());
    assert_eq!(idea.cancelled_from, None);

    let minimal_run = r#"{
        "id": "r-abcd1234",
        "idea": "i-abcd1234",
        "workflow": "default",
        "workflow_version": "1",
        "workflow_sha256": "abc",
        "state": "pending",
        "created": 1.0,
        "updated": 1.0
    }"#;
    let run: Run = serde_json::from_str(minimal_run).unwrap();
    assert!(run.stages.is_empty());
    assert_eq!(run.budget, None);

    let minimal_budget: Budget = serde_json::from_str("{}").unwrap();
    assert_eq!(minimal_budget, Budget::default());

    let minimal_settings: FactorySettings = serde_json::from_str("{}").unwrap();
    assert_eq!(minimal_settings, FactorySettings::default());
}

#[test]
fn test_factory_settings_default() {
    let settings = FactorySettings::default();
    assert_eq!(settings.max_parallel_agents, 3);
    assert!((settings.default_temperature - 0.4).abs() < f64::EPSILON);
}

#[test]
fn test_validators() {
    assert_eq!(validate_title("  Hello world  ").unwrap(), "Hello world");
    assert!(validate_title("").is_err());
    assert!(validate_title("   ").is_err());
    assert!(validate_title(&"a".repeat(121)).is_err());
    assert!(validate_title(&"a".repeat(120)).is_ok());
    assert!(validate_title("line1\nline2").is_err());
    assert!(validate_title("bad\x00char").is_err());
    assert!(validate_title("bad\tchar").is_err());

    assert_eq!(validate_text("").unwrap(), "");
    assert_eq!(validate_text("line1\n\tline2").unwrap(), "line1\n\tline2");
    assert!(validate_text(&"x".repeat(8000)).is_ok());
    assert!(validate_text(&"x".repeat(8001)).is_err());
    assert!(validate_text("bad\x00char").is_err());
    assert!(validate_text("bad\x1bchar").is_err());
    assert!(validate_text("bad\rchar").is_err());

    assert_eq!(validate_owner("alice_1.dev-2").unwrap(), "alice_1.dev-2");
    assert!(validate_owner("").is_err());
    assert!(validate_owner(&"a".repeat(64)).is_ok());
    assert!(validate_owner(&"a".repeat(65)).is_err());
    assert!(validate_owner("alice bob").is_err());
    assert!(validate_owner("alice/bob").is_err());
    assert!(validate_owner("alice\n").is_err());
}
