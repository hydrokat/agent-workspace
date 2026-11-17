use std::path::PathBuf;

pub mod approval;
pub mod artifacts;
pub mod broker;
pub mod commands;
pub mod events;
pub mod executor;
pub mod ids;
pub mod machine;
pub mod model;
pub mod questions;
pub mod run;
pub mod simulate;
pub mod store;
pub mod workflow;

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
