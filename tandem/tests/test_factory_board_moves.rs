use tandem::factory::board::{
    COLUMN_ORDER, Move, MoveGate, MoveVia, column_keys, legal_moves, run_is_active,
};
use tandem::factory::machine::{Command, Facts, apply, decide};
use tandem::factory::model::{Actor, ActorKind, Idea, IdeaState, RunState};

const ALL_STATES: [IdeaState; 9] = [
    IdeaState::Inbox,
    IdeaState::Ideation,
    IdeaState::Planning,
    IdeaState::ReadyForDevelopment,
    IdeaState::Queued,
    IdeaState::InDevelopment,
    IdeaState::Review,
    IdeaState::Delivered,
    IdeaState::Cancelled,
];

const ALL_RUN_STATES: [RunState; 10] = [
    RunState::Pending,
    RunState::Queued,
    RunState::Running,
    RunState::Paused,
    RunState::Review,
    RunState::Succeeded,
    RunState::Delivered,
    RunState::Failed,
    RunState::Cancelled,
    RunState::Withdrawn,
];

fn user() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "user-1".to_string(),
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
        target_recommendation: None,
        cancelled_from: None,
    }
}

fn facts(run_state: Option<RunState>) -> Facts {
    Facts {
        has_feasibility: true,
        feasibility_blocks: false,
        has_plan: true,
        open_blocking_questions: 0,
        has_active_run: run_state.is_some_and(run_is_active),
    }
}

fn command_for(via: MoveVia) -> Command {
    match via {
        MoveVia::Ready => Command::MarkReady,
        MoveVia::RevokeReady => Command::RevokeReady,
        MoveVia::Cancel => Command::Cancel {
            reason: "board move".to_string(),
        },
    }
}

fn vias(moves: &[Move]) -> Vec<MoveVia> {
    moves.iter().map(|m| m.via).collect()
}

#[test]
fn planning_offers_ready_as_a_user_only_gate_and_cancel() {
    let moves = legal_moves(IdeaState::Planning, None, false);
    assert_eq!(vias(&moves), vec![MoveVia::Ready, MoveVia::Cancel]);
    let ready = &moves[0];
    assert_eq!(ready.to, IdeaState::ReadyForDevelopment);
    assert_eq!(ready.gate, Some(MoveGate::UserOnly));
    assert!(ready.needs_confirm, "Gate 2 must be confirmed");
}

#[test]
fn cancel_always_needs_confirmation_and_targets_cancelled() {
    for state in ALL_STATES.into_iter().filter(|s| !s.is_terminal()) {
        let cancel = legal_moves(state, None, false)
            .into_iter()
            .find(|m| m.via == MoveVia::Cancel)
            .unwrap_or_else(|| panic!("{state:?} should offer cancel"));
        assert_eq!(cancel.to, IdeaState::Cancelled);
        assert!(cancel.needs_confirm, "{state:?} cancel needs confirm");
        assert_eq!(cancel.gate, None);
    }
}

#[test]
fn terminal_columns_are_read_only() {
    for state in [IdeaState::Delivered, IdeaState::Cancelled] {
        for run in std::iter::once(None).chain(ALL_RUN_STATES.into_iter().map(Some)) {
            assert!(legal_moves(state, run, false).is_empty(), "{state:?}");
        }
    }
}

#[test]
fn system_driven_columns_only_offer_cancel() {
    for state in [
        IdeaState::Inbox,
        IdeaState::Ideation,
        IdeaState::Queued,
        IdeaState::InDevelopment,
        IdeaState::Review,
    ] {
        assert_eq!(
            vias(&legal_moves(state, None, false)),
            vec![MoveVia::Cancel],
            "{state:?}"
        );
    }
}

#[test]
fn ready_for_development_offers_revoke_only_without_an_active_run() {
    for run in std::iter::once(None).chain(ALL_RUN_STATES.into_iter().map(Some)) {
        let offered = legal_moves(IdeaState::ReadyForDevelopment, run, false)
            .iter()
            .any(|m| m.via == MoveVia::RevokeReady);
        assert_eq!(offered, !run.is_some_and(run_is_active), "run {run:?}");
    }
}

#[test]
fn revoke_goes_back_to_planning_and_is_a_quick_undo_inside_the_grace_window() {
    let inside = legal_moves(IdeaState::ReadyForDevelopment, None, true);
    let outside = legal_moves(IdeaState::ReadyForDevelopment, None, false);
    let revoke_inside = inside
        .iter()
        .find(|m| m.via == MoveVia::RevokeReady)
        .unwrap();
    let revoke_outside = outside
        .iter()
        .find(|m| m.via == MoveVia::RevokeReady)
        .unwrap();
    assert_eq!(revoke_inside.to, IdeaState::Planning);
    assert_eq!(revoke_inside.gate, Some(MoveGate::UserOnly));
    assert!(!revoke_inside.needs_confirm);
    assert!(revoke_outside.needs_confirm);
}

#[test]
fn queued_is_not_offered_a_move_back_to_planning() {
    let moves = legal_moves(IdeaState::Queued, Some(RunState::Queued), false);
    assert!(moves.iter().all(|m| m.to != IdeaState::Planning));
}

#[test]
fn every_offered_move_is_accepted_by_the_machine_and_lands_on_its_target() {
    for state in ALL_STATES {
        for run in std::iter::once(None).chain(ALL_RUN_STATES.into_iter().map(Some)) {
            for mv in legal_moves(state, run, false) {
                let idea = idea_in_state(state);
                let events = decide(Some(&idea), &command_for(mv.via), &user(), &facts(run))
                    .unwrap_or_else(|e| panic!("{state:?}/{run:?} {:?} rejected: {e:?}", mv.via));
                let mut landed = Some(idea);
                for event in &events {
                    landed = apply(landed, event, 200.0);
                }
                let landed = landed.expect("idea survives").state;
                assert_eq!(landed, mv.to, "{state:?}/{run:?} via {:?}", mv.via);
            }
        }
    }
}

#[test]
fn no_user_transition_the_machine_accepts_is_hidden_from_the_board() {
    for state in ALL_STATES {
        for run in std::iter::once(None).chain(ALL_RUN_STATES.into_iter().map(Some)) {
            let offered = vias(&legal_moves(state, run, false));
            for via in [MoveVia::Ready, MoveVia::RevokeReady, MoveVia::Cancel] {
                let idea = idea_in_state(state);
                let accepted = decide(Some(&idea), &command_for(via), &user(), &facts(run)).is_ok();
                assert_eq!(
                    accepted,
                    offered.contains(&via),
                    "{state:?}/{run:?} via {via:?}"
                );
            }
        }
    }
}

#[test]
fn agents_cannot_perform_any_offered_move() {
    for kind in [ActorKind::Orchestrator, ActorKind::System] {
        let actor = Actor {
            kind,
            id: "agent".to_string(),
        };
        for state in ALL_STATES {
            for mv in legal_moves(state, None, false) {
                let idea = idea_in_state(state);
                assert!(
                    decide(Some(&idea), &command_for(mv.via), &actor, &facts(None)).is_err(),
                    "{kind:?} must not {:?} from {state:?}",
                    mv.via
                );
            }
        }
    }
}

#[test]
fn column_order_lists_every_state_once_in_lifecycle_order() {
    assert_eq!(COLUMN_ORDER.len(), ALL_STATES.len());
    for state in ALL_STATES {
        assert_eq!(COLUMN_ORDER.iter().filter(|s| **s == state).count(), 1);
    }
    assert_eq!(
        column_keys(),
        vec![
            "inbox",
            "ideation",
            "planning",
            "ready_for_development",
            "queued",
            "in_development",
            "review",
            "delivered",
            "cancelled",
        ]
    );
}

#[test]
fn moves_serialize_in_snake_case_and_omit_a_missing_gate() {
    let planning = serde_json::to_value(legal_moves(IdeaState::Planning, None, false)).unwrap();
    assert_eq!(planning[0]["via"], "ready");
    assert_eq!(planning[0]["to"], "ready_for_development");
    assert_eq!(planning[0]["gate"], "user_only");
    assert_eq!(planning[0]["needs_confirm"], true);
    assert!(planning[1].get("gate").is_none());
}

fn temp_workspace(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tandem-test-board-moves-{name}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(path.join(".agent-workspace/coordination/queries")).unwrap();
    std::fs::create_dir_all(path.join("specs")).unwrap();
    path
}

#[test]
fn snapshot_cards_carry_moves_and_the_summary_carries_column_order() {
    let ws = temp_workspace("snapshot");
    let store = tandem::factory::store::FactoryStore::new(&ws);
    store.ensure().unwrap();
    let idea = store
        .create_idea("Board idea", "text", &user().id, &user())
        .unwrap();

    let summary = tandem::snapshot::collect_factory_summary(&ws, &[], 200.0);
    let card = summary.ideas.iter().find(|c| c.id == idea.id).unwrap();
    assert_eq!(vias(&card.moves), vec![MoveVia::Cancel]);
    assert_eq!(summary.columns, column_keys());
    let _ = std::fs::remove_dir_all(ws);
}

#[test]
fn snapshots_written_before_moves_existed_still_deserialize() {
    let old_card = serde_json::json!({
        "id": "i-1",
        "title": "Old",
        "state": "planning",
        "owner": "alice",
        "updated": 1.0
    });
    let card: tandem::snapshot::IdeaCard = serde_json::from_value(old_card).unwrap();
    assert!(card.moves.is_empty());
    let old_summary = serde_json::json!({ "enabled": true });
    let summary: tandem::snapshot::FactorySummary = serde_json::from_value(old_summary).unwrap();
    assert!(summary.columns.is_empty());
}
