use std::path::PathBuf;

pub mod approval;
pub mod artifacts;
pub mod audit;
pub mod board;
pub mod broker;
pub mod budget;
pub mod checks;
pub mod commands;
pub mod data_policy;
pub mod events;
pub mod evidence;
pub mod executor;
pub mod handoff;
pub mod ids;
pub mod login;
pub mod machine;
pub mod model;
pub mod model_catalog;
pub mod permissions;
pub mod pr;
pub mod protected;
pub mod questions;
pub mod recovery;
pub mod redact;
pub mod routing;
pub mod run;
pub mod runner;
pub mod settings;
pub mod simulate;
pub mod stages;
pub mod store;
pub mod team;
pub mod telemetry;
pub mod ui_intent;
pub mod worker;
pub mod workflow;
pub mod workspace;

use machine::TransitionError;

#[derive(Debug, thiserror::Error)]
pub enum FactoryError {
    #[error("refusing to follow symlink at {}", .0.display())]
    Symlink(PathBuf),
    #[error("event size {bytes} bytes exceeds {max} byte limit")]
    EventTooLarge { bytes: usize, max: usize },
    #[error("{0}")]
    InvalidInput(String),
    #[error(transparent)]
    Transition(#[from] TransitionError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
