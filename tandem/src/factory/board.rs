use serde::{Deserialize, Serialize};

use crate::factory::model::{IdeaState, RunState};

pub const COLUMN_ORDER: [IdeaState; 9] = [
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveVia {
    Ready,
    RevokeReady,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveGate {
    UserOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    pub to: IdeaState,
    pub via: MoveVia,
    pub needs_confirm: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<MoveGate>,
}

pub fn run_is_active(state: RunState) -> bool {
    matches!(
        state,
        RunState::Pending
            | RunState::Queued
            | RunState::Running
            | RunState::Paused
            | RunState::Review
    )
}

pub fn state_key(state: IdeaState) -> &'static str {
    match state {
        IdeaState::Inbox => "inbox",
        IdeaState::Ideation => "ideation",
        IdeaState::Planning => "planning",
        IdeaState::ReadyForDevelopment => "ready_for_development",
        IdeaState::Queued => "queued",
        IdeaState::InDevelopment => "in_development",
        IdeaState::Review => "review",
        IdeaState::Delivered => "delivered",
        IdeaState::Cancelled => "cancelled",
    }
}

pub fn column_keys() -> Vec<&'static str> {
    COLUMN_ORDER.iter().map(|s| state_key(*s)).collect()
}

/// Moves a user may make from `state`. Only transitions the gated commands
/// (`idea ready`, `idea revoke-ready`, `idea cancel`) already accept are listed.
/// `within_grace` marks the approval grace window, where revoking is a quick undo.
pub fn legal_moves(state: IdeaState, run_state: Option<RunState>, within_grace: bool) -> Vec<Move> {
    if state.is_terminal() {
        return Vec::new();
    }
    let mut moves = Vec::new();
    match state {
        IdeaState::Planning => moves.push(Move {
            to: IdeaState::ReadyForDevelopment,
            via: MoveVia::Ready,
            needs_confirm: true,
            gate: Some(MoveGate::UserOnly),
        }),
        IdeaState::ReadyForDevelopment if !run_state.is_some_and(run_is_active) => {
            moves.push(Move {
                to: IdeaState::Planning,
                via: MoveVia::RevokeReady,
                needs_confirm: !within_grace,
                gate: Some(MoveGate::UserOnly),
            })
        }
        _ => {}
    }
    moves.push(Move {
        to: IdeaState::Cancelled,
        via: MoveVia::Cancel,
        needs_confirm: true,
        gate: None,
    });
    moves
}
