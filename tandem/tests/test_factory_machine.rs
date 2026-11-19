use tandem::factory::machine::{Command, EventData, Facts, TransitionError, apply, decide};
use tandem::factory::model::{Actor, ActorKind, ApprovalGate, Idea, IdeaState, PrLink};

fn actor(kind: ActorKind) -> Actor {
    Actor {
        kind,
        id: match kind {
            ActorKind::User => "user-1".to_string(),
            ActorKind::Orchestrator => "orch-1".to_string(),
            ActorKind::System => "system".to_string(),
        },
    }
}

fn permissive_facts() -> Facts {
    Facts {
        has_feasibility: true,
        feasibility_blocks: false,
        has_plan: true,
        open_blocking_questions: 0,
        has_active_run: false,
    }
}

fn sample_pr() -> PrLink {
    PrLink {
        repo: "agent-workspace-package".to_string(),
        number: 42,
        url: "https://github.com/example/repo/pull/42".to_string(),
        state: None,
        checks_summary: None,
    }
}

fn idea_in_state(state: IdeaState) -> Idea {
    Idea {
        id: "i-12345678".to_string(),
        title: "Sample idea".to_string(),
        text: "Idea body".to_string(),
        state,
        owner: "alice".to_string(),
        created: 100.0,
        updated: 100.0,
        risk: None,
        orchestrator: None,
        feasibility: None,
        plan: None,
        plan_snapshot: None,
        ready_via: None,
        ready_at: None,
        feasibility_verdict: None,
        artifacts: Vec::new(),
        assumptions: Vec::new(),
        runs: Vec::new(),
        prs: Vec::new(),
        cancelled_from: None,
    }
}

fn all_states() -> [IdeaState; 9] {
    [
        IdeaState::Inbox,
        IdeaState::Ideation,
        IdeaState::Planning,
        IdeaState::ReadyForDevelopment,
        IdeaState::Queued,
        IdeaState::InDevelopment,
        IdeaState::Review,
        IdeaState::Delivered,
        IdeaState::Cancelled,
    ]
}

fn all_actors() -> [ActorKind; 3] {
    [ActorKind::User, ActorKind::Orchestrator, ActorKind::System]
}

fn all_commands() -> Vec<Command> {
    vec![
        Command::Create {
            id: "i-12345678".to_string(),
            title: "Sample idea".to_string(),
            text: "Idea body".to_string(),
            owner: "alice".to_string(),
        },
        Command::StartDiscovery,
        Command::SelectDirection,
        Command::MarkReady,
        Command::RevokeReady,
        Command::CreateRun {
            run: "r-12345678".to_string(),
        },
        Command::StartRun {
            run: "r-12345678".to_string(),
        },
        Command::StagesComplete {
            run: "r-12345678".to_string(),
        },
        Command::ChangesRequested {
            run: "r-12345678".to_string(),
        },
        Command::Deliver {
            run: "r-12345678".to_string(),
            prs: vec![sample_pr()],
        },
        Command::Cancel {
            reason: "no longer needed".to_string(),
        },
    ]
}

#[derive(Debug, PartialEq, Eq)]
enum ExpectedOutcome {
    Ok(IdeaState),
    NotFound,
    AlreadyExists,
    IllegalTransition,
    ActorNotAllowed,
}

fn expected_for_triple(from: Option<IdeaState>, cmd: &Command, kind: ActorKind) -> ExpectedOutcome {
    match (from, cmd) {
        (None, Command::Create { .. }) => match kind {
            ActorKind::User | ActorKind::Orchestrator => ExpectedOutcome::Ok(IdeaState::Inbox),
            ActorKind::System => ExpectedOutcome::ActorNotAllowed,
        },
        (None, _) => ExpectedOutcome::NotFound,
        (Some(_), Command::Create { .. }) => ExpectedOutcome::AlreadyExists,
        (Some(IdeaState::Inbox), Command::StartDiscovery) => match kind {
            ActorKind::User | ActorKind::Orchestrator => ExpectedOutcome::Ok(IdeaState::Ideation),
            ActorKind::System => ExpectedOutcome::ActorNotAllowed,
        },
        (Some(IdeaState::Ideation), Command::SelectDirection) => match kind {
            ActorKind::User | ActorKind::Orchestrator => ExpectedOutcome::Ok(IdeaState::Planning),
            ActorKind::System => ExpectedOutcome::ActorNotAllowed,
        },
        (Some(IdeaState::Planning), Command::MarkReady) => match kind {
            ActorKind::User => ExpectedOutcome::Ok(IdeaState::ReadyForDevelopment),
            ActorKind::Orchestrator | ActorKind::System => ExpectedOutcome::ActorNotAllowed,
        },
        (Some(IdeaState::ReadyForDevelopment), Command::RevokeReady) => match kind {
            ActorKind::User => ExpectedOutcome::Ok(IdeaState::Planning),
            ActorKind::Orchestrator | ActorKind::System => ExpectedOutcome::ActorNotAllowed,
        },
        (Some(IdeaState::ReadyForDevelopment), Command::CreateRun { .. }) => match kind {
            ActorKind::System => ExpectedOutcome::Ok(IdeaState::Queued),
            ActorKind::User | ActorKind::Orchestrator => ExpectedOutcome::ActorNotAllowed,
        },
        (Some(IdeaState::Queued), Command::StartRun { .. }) => match kind {
            ActorKind::System => ExpectedOutcome::Ok(IdeaState::InDevelopment),
            ActorKind::User | ActorKind::Orchestrator => ExpectedOutcome::ActorNotAllowed,
        },
        (Some(IdeaState::InDevelopment), Command::StagesComplete { .. }) => match kind {
            ActorKind::System => ExpectedOutcome::Ok(IdeaState::Review),
            ActorKind::User | ActorKind::Orchestrator => ExpectedOutcome::ActorNotAllowed,
        },
        (Some(IdeaState::Review), Command::ChangesRequested { .. }) => match kind {
            ActorKind::System => ExpectedOutcome::Ok(IdeaState::InDevelopment),
            ActorKind::User | ActorKind::Orchestrator => ExpectedOutcome::ActorNotAllowed,
        },
        (Some(IdeaState::Review), Command::Deliver { .. }) => match kind {
            ActorKind::System => ExpectedOutcome::Ok(IdeaState::Delivered),
            ActorKind::User | ActorKind::Orchestrator => ExpectedOutcome::ActorNotAllowed,
        },
        (Some(state), Command::Cancel { .. }) if !state.is_terminal() => match kind {
            ActorKind::User => ExpectedOutcome::Ok(IdeaState::Cancelled),
            ActorKind::Orchestrator | ActorKind::System => ExpectedOutcome::ActorNotAllowed,
        },
        (Some(_), _) => ExpectedOutcome::IllegalTransition,
    }
}

#[test]
fn test_exhaustive_transition_table_and_apply_to_state() {
    let facts = permissive_facts();
    let mut from_states: Vec<Option<IdeaState>> = vec![None];
    from_states.extend(all_states().into_iter().map(Some));

    for from in from_states {
        for cmd in all_commands() {
            for kind in all_actors() {
                let current = from.map(idea_in_state);
                let result = decide(current.as_ref(), &cmd, &actor(kind), &facts);
                let expected = expected_for_triple(from, &cmd, kind);

                match (expected, result) {
                    (ExpectedOutcome::Ok(to_state), Ok(events)) => {
                        assert!(
                            !events.is_empty(),
                            "expected events for ({from:?}, {cmd:?}, {kind:?})"
                        );
                        let next = events.iter().fold(current, |acc, ev| apply(acc, ev, 200.0));
                        let next_idea =
                            next.expect("apply after valid decide must yield Some(Idea)");
                        assert_eq!(
                            next_idea.state, to_state,
                            "wrong target state for ({from:?}, {cmd:?}, {kind:?})"
                        );
                        if matches!(cmd, Command::Cancel { .. }) {
                            assert_eq!(next_idea.cancelled_from, from);
                        }
                    }
                    (ExpectedOutcome::NotFound, Err(TransitionError::NotFound)) => {}
                    (ExpectedOutcome::AlreadyExists, Err(TransitionError::AlreadyExists)) => {}
                    (
                        ExpectedOutcome::IllegalTransition,
                        Err(TransitionError::IllegalTransition { from: err_from, .. }),
                    ) => {
                        assert_eq!(Some(err_from), from);
                    }
                    (
                        ExpectedOutcome::ActorNotAllowed,
                        Err(TransitionError::ActorNotAllowed {
                            actor: err_actor, ..
                        }),
                    ) => {
                        assert_eq!(err_actor, kind);
                    }
                    (exp, got) => {
                        panic!(
                            "mismatch for ({from:?}, {cmd:?}, {kind:?}): expected {exp:?}, got {got:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_gate_1_feasibility_guards() {
    let idea = idea_in_state(IdeaState::Ideation);
    let user = actor(ActorKind::User);

    let missing_feasibility = Facts {
        has_feasibility: false,
        feasibility_blocks: false,
        ..Default::default()
    };
    assert!(matches!(
        decide(
            Some(&idea),
            &Command::SelectDirection,
            &user,
            &missing_feasibility
        ),
        Err(TransitionError::GateNotMet {
            gate: ApprovalGate::Feasibility,
            ..
        })
    ));

    let blocked_feasibility = Facts {
        has_feasibility: true,
        feasibility_blocks: true,
        ..Default::default()
    };
    assert!(matches!(
        decide(
            Some(&idea),
            &Command::SelectDirection,
            &user,
            &blocked_feasibility
        ),
        Err(TransitionError::GateNotMet {
            gate: ApprovalGate::Feasibility,
            ..
        })
    ));

    let passing = Facts {
        has_feasibility: true,
        feasibility_blocks: false,
        ..Default::default()
    };
    assert!(decide(Some(&idea), &Command::SelectDirection, &user, &passing).is_ok());
}

#[test]
fn test_gate_2_mark_ready_guards_and_user_only_restriction() {
    let idea = idea_in_state(IdeaState::Planning);
    let user = actor(ActorKind::User);

    let missing_plan = Facts {
        has_plan: false,
        open_blocking_questions: 0,
        ..Default::default()
    };
    assert!(matches!(
        decide(Some(&idea), &Command::MarkReady, &user, &missing_plan),
        Err(TransitionError::GateNotMet {
            gate: ApprovalGate::ReadyForDevelopment,
            ..
        })
    ));

    let open_questions = Facts {
        has_plan: true,
        open_blocking_questions: 2,
        ..Default::default()
    };
    assert!(matches!(
        decide(Some(&idea), &Command::MarkReady, &user, &open_questions),
        Err(TransitionError::GateNotMet {
            gate: ApprovalGate::ReadyForDevelopment,
            ..
        })
    ));

    let passing = Facts {
        has_plan: true,
        open_blocking_questions: 0,
        ..Default::default()
    };
    assert!(decide(Some(&idea), &Command::MarkReady, &user, &passing).is_ok());

    for non_user in [ActorKind::Orchestrator, ActorKind::System] {
        for facts in [
            missing_plan.clone(),
            open_questions.clone(),
            passing.clone(),
        ] {
            assert!(
                matches!(
                    decide(Some(&idea), &Command::MarkReady, &actor(non_user), &facts),
                    Err(TransitionError::ActorNotAllowed {
                        actor: kind,
                        ..
                    }) if kind == non_user
                ),
                "MarkReady by {non_user:?} must always be ActorNotAllowed"
            );
        }
    }
}

#[test]
fn test_revoke_ready_and_deliver_and_create_guards() {
    let ready = idea_in_state(IdeaState::ReadyForDevelopment);
    let active_run_facts = Facts {
        has_active_run: true,
        ..permissive_facts()
    };
    assert!(matches!(
        decide(
            Some(&ready),
            &Command::RevokeReady,
            &actor(ActorKind::User),
            &active_run_facts
        ),
        Err(TransitionError::InvalidInput(_))
    ));

    let review = idea_in_state(IdeaState::Review);
    assert!(matches!(
        decide(
            Some(&review),
            &Command::Deliver {
                run: "r-12345678".to_string(),
                prs: vec![],
            },
            &actor(ActorKind::System),
            &permissive_facts(),
        ),
        Err(TransitionError::InvalidInput(_))
    ));

    assert!(matches!(
        decide(
            None,
            &Command::Create {
                id: "i-12345678".to_string(),
                title: "   ".to_string(),
                text: "body".to_string(),
                owner: "alice".to_string(),
            },
            &actor(ActorKind::User),
            &permissive_facts(),
        ),
        Err(TransitionError::InvalidInput(_))
    ));

    assert!(matches!(
        decide(
            None,
            &Command::Create {
                id: "i-12345678".to_string(),
                title: "Valid".to_string(),
                text: "body".to_string(),
                owner: "bad owner".to_string(),
            },
            &actor(ActorKind::User),
            &permissive_facts(),
        ),
        Err(TransitionError::InvalidInput(_))
    ));
}

#[test]
fn test_replay_determinism_and_apply_totality() {
    let facts = permissive_facts();
    let user = actor(ActorKind::User);
    let orch = actor(ActorKind::Orchestrator);
    let sys = actor(ActorKind::System);

    let steps: Vec<(Command, &Actor, f64)> = vec![
        (
            Command::Create {
                id: "i-12345678".to_string(),
                title: "  Ship factory  ".to_string(),
                text: "Details".to_string(),
                owner: "alice".to_string(),
            },
            &user,
            10.0,
        ),
        (Command::StartDiscovery, &orch, 20.0),
        (Command::SelectDirection, &orch, 30.0),
        (Command::MarkReady, &user, 40.0),
        (
            Command::CreateRun {
                run: "r-11111111".to_string(),
            },
            &sys,
            50.0,
        ),
        (
            Command::StartRun {
                run: "r-11111111".to_string(),
            },
            &sys,
            60.0,
        ),
        (
            Command::StagesComplete {
                run: "r-11111111".to_string(),
            },
            &sys,
            70.0,
        ),
        (
            Command::ChangesRequested {
                run: "r-11111111".to_string(),
            },
            &sys,
            80.0,
        ),
        (
            Command::StagesComplete {
                run: "r-11111111".to_string(),
            },
            &sys,
            90.0,
        ),
        (
            Command::Deliver {
                run: "r-11111111".to_string(),
                prs: vec![sample_pr()],
            },
            &sys,
            100.0,
        ),
    ];

    let mut log: Vec<(EventData, f64)> = Vec::new();
    let mut state: Option<Idea> = None;
    for (cmd, who, ts) in steps {
        let events = decide(state.as_ref(), &cmd, who, &facts).unwrap();
        for ev in events {
            state = apply(state, &ev, ts);
            log.push((ev, ts));
        }
    }

    let first_replay = log
        .iter()
        .fold(None, |acc, (ev, ts)| apply(acc, ev, *ts))
        .expect("replay must produce Idea");
    let second_replay = log
        .iter()
        .fold(None, |acc, (ev, ts)| apply(acc, ev, *ts))
        .expect("replay must produce Idea");

    assert_eq!(first_replay, second_replay);
    assert_eq!(first_replay.state, IdeaState::Delivered);
    assert_eq!(first_replay.title, "Ship factory");
    assert_eq!(first_replay.runs, vec!["r-11111111".to_string()]);
    assert_eq!(first_replay.prs, vec![sample_pr()]);

    assert_eq!(apply(None, &EventData::IdeaDiscoveryStarted {}, 1.0), None);
    let unchanged = apply(
        Some(first_replay.clone()),
        &EventData::IdeaCancelled {
            reason: "late".to_string(),
        },
        200.0,
    );
    assert_eq!(unchanged, Some(first_replay));
}

#[test]
fn test_event_data_serde_envelope_fields() {
    let events = vec![
        (
            EventData::IdeaCreated {
                id: "i-12345678".to_string(),
                title: "Title".to_string(),
                text: "Text".to_string(),
                owner: "alice".to_string(),
            },
            "idea.created",
        ),
        (EventData::IdeaDiscoveryStarted {}, "idea.discovery_started"),
        (
            EventData::IdeaDirectionSelected {},
            "idea.direction_selected",
        ),
        (EventData::IdeaReady {}, "idea.ready"),
        (EventData::IdeaReadyRevoked {}, "idea.ready_revoked"),
        (
            EventData::RunCreated {
                run: "r-12345678".to_string(),
                workflow: String::new(),
                version: String::new(),
                sha256: String::new(),
                source: String::new(),
                plan_sha256: String::new(),
                deploy_requested: false,
                stages: Vec::new(),
                budget: None,
            },
            "run.created",
        ),
        (
            EventData::RunStarted {
                run: "r-12345678".to_string(),
            },
            "run.started",
        ),
        (
            EventData::RunStagesComplete {
                run: "r-12345678".to_string(),
            },
            "run.stages_complete",
        ),
        (
            EventData::RunChangesRequested {
                run: "r-12345678".to_string(),
            },
            "run.changes_requested",
        ),
        (
            EventData::IdeaDelivered {
                run: "r-12345678".to_string(),
                prs: vec![sample_pr()],
            },
            "idea.delivered",
        ),
        (
            EventData::IdeaCancelled {
                reason: "done".to_string(),
            },
            "idea.cancelled",
        ),
    ];

    for (ev, expected_type) in events {
        let json = serde_json::to_value(&ev).expect("serialize EventData");
        assert_eq!(
            json.get("type").and_then(|v| v.as_str()),
            Some(expected_type)
        );
        assert!(
            json.get("data").is_some(),
            "missing data object for {expected_type}"
        );
        let round_trip: EventData = serde_json::from_value(json).expect("deserialize EventData");
        assert_eq!(round_trip, ev);
    }
}

#[test]
fn stage_reopened_for_develop_resets_test_review_and_demo() {
    use tandem::factory::machine::apply_run;
    use tandem::factory::model::{Run, RunState, Stage, StageState};

    let make_stage = |name: &str, state: StageState| Stage {
        name: name.to_string(),
        agent: "agent".to_string(),
        state,
        attempts: 1,
        interrupted_attempts: 0,
        started: Some(1.0),
        finished: Some(2.0),
        artifacts: vec![],
        rationale: None,
        retry_hypothesis: None,
    };

    let run = Run {
        id: "r-12345678".to_string(),
        idea: "i-12345678".to_string(),
        workflow: "default".to_string(),
        workflow_version: "1".to_string(),
        workflow_sha256: "abc".to_string(),
        workflow_source: "test".to_string(),
        plan_sha256: "def".to_string(),
        state: RunState::Running,
        stages: vec![
            make_stage("develop", StageState::Succeeded),
            make_stage("test", StageState::Succeeded),
            make_stage("review", StageState::Failed),
            make_stage("demo", StageState::Succeeded),
        ],
        created: 1.0,
        updated: 8.0,
        pause_reason: None,
        action_commands: vec![],
        worktree_path: None,
        branch: None,
        target: None,
        base_commit: None,
        baseline_passed: None,
        deploy_requested: false,
        evidence: vec![],
        prs: vec![],
        budget: None,
    };

    let updated = apply_run(
        Some(run),
        &EventData::StageReopened {
            run: "r-12345678".to_string(),
            stage: "develop".to_string(),
            reason: "review requested changes".to_string(),
        },
        "i-12345678",
        9.0,
    )
    .expect("apply_run must return updated Run");

    for name in ["develop", "test", "review", "demo"] {
        let st = updated.stages.iter().find(|s| s.name == name).unwrap();
        assert_eq!(
            st.state,
            StageState::Pending,
            "stage {name} should be reset to Pending"
        );
        assert_eq!(st.finished, None, "stage {name} finished should be cleared");
    }
}
