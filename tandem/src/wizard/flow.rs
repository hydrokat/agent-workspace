use std::collections::HashMap;
use std::path::PathBuf;

use crate::adapters::base::{AgentSpec, validate_model_for_cli};

pub use crate::state::CLIS;
pub const ROLES: [&str; 2] = ["planner", "implementer"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Cli,
    Role,
    Model,
    Perms,
    Cwd,
    Summary,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WizardError {
    #[error("{0} is not installed")]
    NotInstalled(String),
    #[error("role must be planner or implementer")]
    InvalidRole,
    #[error("invalid model: {0}")]
    InvalidModel(String),
    #[error("directory does not exist")]
    DirectoryNotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WizardFlow {
    pub installed: HashMap<String, bool>,
    pub agents: Vec<AgentSpec>,
    step: Step,
    cli: String,
    role: String,
    model: String,
    skip: bool,
}

impl WizardFlow {
    pub fn new(installed: HashMap<String, bool>) -> Self {
        Self {
            installed,
            agents: Vec::new(),
            step: Step::Cli,
            cli: String::new(),
            role: "planner".to_string(),
            model: String::new(),
            skip: false,
        }
    }

    pub fn step(&self) -> Step {
        self.step
    }

    pub fn can_launch(&self) -> bool {
        !self.agents.is_empty()
    }

    pub fn draft_cli(&self) -> &str {
        &self.cli
    }

    pub fn draft_role(&self) -> &str {
        &self.role
    }

    pub fn choose_cli(&mut self, cli: &str) -> Result<(), WizardError> {
        let is_installed = self.installed.get(cli).copied().unwrap_or(false);
        if !CLIS.contains(&cli) || !is_installed {
            return Err(WizardError::NotInstalled(cli.to_string()));
        }
        self.cli = cli.to_string();
        self.step = Step::Role;
        Ok(())
    }

    pub fn choose_role(&mut self, role: &str) -> Result<(), WizardError> {
        if !ROLES.contains(&role) {
            return Err(WizardError::InvalidRole);
        }
        self.role = role.to_string();
        self.step = Step::Model;
        Ok(())
    }

    pub fn choose_model(&mut self, model: &str) -> Result<(), WizardError> {
        let validated = validate_model_for_cli(&self.cli, model)
            .map_err(|_| WizardError::InvalidModel(model.to_string()))?;
        self.model = validated;
        self.skip = false;
        if self.asks_permissions() {
            self.step = Step::Perms;
        } else {
            self.after_perms();
        }
        Ok(())
    }

    fn asks_permissions(&self) -> bool {
        matches!(self.cli.as_str(), "agy" | "opencode")
    }

    pub fn choose_skip_permissions(&mut self, skip: bool) {
        self.skip = skip;
        self.after_perms();
    }

    pub fn choose_cwd(&mut self, text: &str) -> Result<(), WizardError> {
        let trimmed = text.trim();
        let cwd: Option<String> = if trimmed.is_empty() {
            None
        } else {
            let path = if let Some(stripped) = trimmed.strip_prefix("~/") {
                if let Ok(home) = std::env::var("HOME") {
                    PathBuf::from(home).join(stripped)
                } else {
                    PathBuf::from(trimmed)
                }
            } else {
                PathBuf::from(trimmed)
            };
            if !path.is_dir() {
                return Err(WizardError::DirectoryNotFound);
            }
            Some(path.to_string_lossy().into_owned())
        };
        self.commit(cwd);
        Ok(())
    }

    fn after_perms(&mut self) {
        if self.role == "implementer" {
            self.step = Step::Cwd;
        } else {
            self.commit(None);
        }
    }

    fn commit(&mut self, cwd: Option<String>) {
        let index = self.agents.iter().filter(|a| a.role == self.role).count() + 1;
        let mut spec = AgentSpec::new(&self.cli, &self.role, &self.model);
        spec.index = index;
        spec.skip_permissions = self.skip;
        spec.cwd = cwd;
        self.agents.push(spec);
        self.step = Step::Summary;
    }

    pub fn add_another(&mut self) {
        self.step = Step::Cli;
    }

    pub fn delete(&mut self, position: usize) {
        if position < self.agents.len() {
            self.agents.remove(position);
            self.renumber();
        }
    }

    pub fn edit(&mut self, position: usize) {
        self.delete(position);
        self.step = Step::Cli;
    }

    fn renumber(&mut self) {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for a in &mut self.agents {
            let count = counts.entry(a.role.clone()).or_insert(0);
            *count += 1;
            a.index = *count;
        }
    }

    pub fn back(&mut self) {
        match self.step {
            Step::Cli => {
                if !self.agents.is_empty() {
                    self.step = Step::Summary;
                }
            }
            Step::Role => {
                self.step = Step::Cli;
            }
            Step::Model => {
                self.step = Step::Role;
            }
            Step::Perms => {
                self.step = Step::Model;
            }
            Step::Cwd => {
                if self.asks_permissions() {
                    self.step = Step::Perms;
                } else {
                    self.step = Step::Model;
                }
            }
            Step::Summary => {}
        }
    }
}
