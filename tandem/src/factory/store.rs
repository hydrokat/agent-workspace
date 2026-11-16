use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub use super::FactoryError;
use super::events::{
    self, Event, append_locked, read_all, reject_if_symlink, set_private_dir_mode,
    set_private_file_mode,
};
use super::ids::{self, is_valid_idea_id};
use super::machine::{self, Command, EventData, Facts};
use super::model::{Actor, FactorySettings, Idea};
use crate::coordination::filelock::exclusive_lock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebuildReport {
    pub ideas: usize,
    pub truncated: bool,
    pub skipped: usize,
}

#[derive(Debug, Clone)]
pub struct FactoryStore {
    pub root: PathBuf,
}

impl FactoryStore {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            root: workspace.into().join(".agent-workspace").join("factory"),
        }
    }

    pub fn exists(&self) -> bool {
        self.root.is_dir() && !self.root.is_symlink()
    }

    fn ideas_dir(&self) -> PathBuf {
        self.root.join("ideas")
    }

    fn events_path(&self) -> PathBuf {
        self.root.join("events.jsonl")
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join(".lock")
    }

    fn gitignore_path(&self) -> PathBuf {
        self.root.join(".gitignore")
    }

    fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    fn idea_path(&self, id: &str) -> PathBuf {
        self.ideas_dir().join(format!("{id}.json"))
    }

    pub fn ensure(&self) -> Result<(), FactoryError> {
        if let Some(parent) = self.root.parent() {
            reject_if_symlink(parent)?;
        }
        reject_if_symlink(&self.root)?;
        reject_if_symlink(&self.ideas_dir())?;
        reject_if_symlink(&self.events_path())?;
        reject_if_symlink(&self.gitignore_path())?;
        reject_if_symlink(&self.lock_path())?;

        create_private_dir(&self.root)?;
        create_private_dir(&self.ideas_dir())?;

        let gitignore = self.gitignore_path();
        if !gitignore.exists() {
            write_atomic(&gitignore, b"*\n")?;
        }

        Ok(())
    }

    pub fn create_idea(
        &self,
        title: &str,
        text: &str,
        owner: &str,
        actor: &Actor,
    ) -> Result<Idea, FactoryError> {
        let id = ids::new_idea_id();
        self.execute(
            &id,
            Command::Create {
                id: id.clone(),
                title: title.to_string(),
                text: text.to_string(),
                owner: owner.to_string(),
            },
            actor,
        )
    }

    pub fn execute(
        &self,
        idea_id: &str,
        command: Command,
        actor: &Actor,
    ) -> Result<Idea, FactoryError> {
        if !is_valid_idea_id(idea_id) {
            return Err(FactoryError::InvalidInput(format!(
                "invalid idea id: {idea_id}"
            )));
        }

        self.ensure()?;

        let lock_path = self.lock_path();
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;
        set_private_file_mode(&lock_path)?;
        let _guard = exclusive_lock(&lock_file)?;

        let current = self.load_idea_for_update(idea_id)?;
        let facts = Facts {
            has_feasibility: current.as_ref().is_some_and(|i| i.feasibility.is_some()),
            feasibility_blocks: current.as_ref().is_some_and(|i| {
                i.feasibility_verdict == Some(super::model::FeasibilityVerdict::Infeasible)
            }),
            has_plan: current.as_ref().is_some_and(|i| i.plan.is_some()),
            open_blocking_questions: 0,
            has_active_run: false,
        };
        let decided = machine::decide(current.as_ref(), &command, actor, &facts)?;

        let ts = current_time();
        let mut pending = Vec::with_capacity(decided.len());
        for ev_data in &decided {
            pending.push(Event::from_data(
                ts,
                actor.clone(),
                idea_id.to_string(),
                event_run_id(ev_data),
                ev_data,
            )?);
        }

        let appended = append_locked(&self.events_path(), &pending)?;

        let mut next = current;
        for (ev, ev_data) in appended.iter().zip(decided.iter()) {
            next = machine::apply(next, ev_data, ev.ts);
        }

        let updated = next.ok_or_else(|| {
            FactoryError::InvalidInput("command did not produce an idea state".to_string())
        })?;

        let serialized = serde_json::to_vec_pretty(&updated)?;
        write_atomic(&self.idea_path(idea_id), &serialized)?;

        Ok(updated)
    }

    pub fn get_idea(&self, id: &str) -> Result<Option<Idea>, FactoryError> {
        if !is_valid_idea_id(id) {
            return Err(FactoryError::InvalidInput(format!("invalid idea id: {id}")));
        }
        reject_if_symlink(&self.root)?;
        reject_if_symlink(&self.ideas_dir())?;
        reject_if_symlink(&self.events_path())?;

        let path = self.idea_path(id);
        reject_if_symlink(&path)?;

        if !self.exists() {
            return Ok(None);
        }

        if let Ok(content) = fs::read_to_string(&path)
            && let Ok(idea) = serde_json::from_str::<Idea>(&content)
        {
            return Ok(Some(idea));
        }

        let log = read_all(&self.events_path())?;
        Ok(replay_single_idea(&log.events, id))
    }

    pub fn list_ideas(&self) -> Result<Vec<Idea>, FactoryError> {
        reject_if_symlink(&self.root)?;
        reject_if_symlink(&self.ideas_dir())?;
        reject_if_symlink(&self.events_path())?;

        if !self.exists() {
            return Ok(Vec::new());
        }

        let dir = self.ideas_dir();
        if !dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut paths = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            reject_if_symlink(&path)?;
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                paths.push(path);
            }
        }
        paths.sort();

        let mut cached_log: Option<events::ReadResult> = None;
        let mut ideas = Vec::new();

        for path in paths {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if !is_valid_idea_id(stem) {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(idea) = serde_json::from_str::<Idea>(&content)
            {
                ideas.push(idea);
                continue;
            }

            if cached_log.is_none() {
                cached_log = Some(read_all(&self.events_path())?);
            }
            if let Some(log) = cached_log.as_ref()
                && let Some(rebuilt) = replay_single_idea(&log.events, stem)
            {
                ideas.push(rebuilt);
            }
        }

        ideas.sort_by(|a, b| b.updated.partial_cmp(&a.updated).unwrap_or(Ordering::Equal));
        Ok(ideas)
    }

    pub fn rebuild(&self) -> Result<RebuildReport, FactoryError> {
        self.ensure()?;

        let lock_path = self.lock_path();
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;
        set_private_file_mode(&lock_path)?;
        let _guard = exclusive_lock(&lock_file)?;

        let log = read_all(&self.events_path())?;
        let mut by_id: BTreeMap<String, Option<Idea>> = BTreeMap::new();

        for ev in &log.events {
            let Ok(ev_data) = ev.to_event_data() else {
                continue;
            };
            let entry = by_id.entry(ev.idea.clone()).or_insert(None);
            *entry = machine::apply(entry.take(), &ev_data, ev.ts);
        }

        let mut rebuilt = 0usize;
        for (id, idea_opt) in by_id {
            if let Some(idea) = idea_opt {
                if !is_valid_idea_id(&id) {
                    continue;
                }
                let serialized = serde_json::to_vec_pretty(&idea)?;
                write_atomic(&self.idea_path(&id), &serialized)?;
                rebuilt += 1;
            }
        }

        Ok(RebuildReport {
            ideas: rebuilt,
            truncated: log.truncated,
            skipped: log.skipped,
        })
    }

    pub fn settings(&self) -> Result<FactorySettings, FactoryError> {
        reject_if_symlink(&self.root)?;
        let path = self.settings_path();
        reject_if_symlink(&path)?;

        match fs::read_to_string(&path) {
            Ok(content) => Ok(serde_json::from_str(&content)?),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(FactorySettings::default())
            }
            Err(err) => Err(FactoryError::Io(err)),
        }
    }

    fn load_idea_for_update(&self, idea_id: &str) -> Result<Option<Idea>, FactoryError> {
        let path = self.idea_path(idea_id);
        reject_if_symlink(&path)?;

        let materialized = fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<Idea>(&content).ok());

        let log = read_all(&self.events_path())?;
        let latest_log_ts = log
            .events
            .iter()
            .filter(|ev| ev.idea == idea_id)
            .map(|ev| ev.ts)
            .reduce(f64::max);

        match (materialized, latest_log_ts) {
            (Some(idea), Some(log_ts)) if idea.updated >= log_ts => Ok(Some(idea)),
            (Some(idea), None) => Ok(Some(idea)),
            _ => Ok(replay_single_idea(&log.events, idea_id)),
        }
    }
}

fn replay_single_idea(events: &[Event], idea_id: &str) -> Option<Idea> {
    let mut acc = None;
    for ev in events {
        if ev.idea != idea_id {
            continue;
        }
        if let Ok(ev_data) = ev.to_event_data() {
            acc = machine::apply(acc, &ev_data, ev.ts);
        }
    }
    acc
}

fn event_run_id(ev: &EventData) -> Option<String> {
    match ev {
        EventData::RunCreated { run }
        | EventData::RunStarted { run }
        | EventData::RunStagesComplete { run }
        | EventData::RunChangesRequested { run }
        | EventData::IdeaDelivered { run, .. } => Some(run.clone()),
        _ => None,
    }
}

fn create_private_dir(path: &Path) -> Result<(), FactoryError> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    set_private_dir_mode(path)?;
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), FactoryError> {
    reject_if_symlink(path)?;
    let tmp_path = path.with_extension(
        path.extension()
            .and_then(|s| s.to_str())
            .map(|ext| format!("{ext}.tmp"))
            .unwrap_or_else(|| "tmp".to_string()),
    );
    reject_if_symlink(&tmp_path)?;

    let mut file = File::create(&tmp_path)?;
    set_private_file_mode(&tmp_path)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.sync_all()?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn current_time() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_millis() as f64) / 1000.0)
        .unwrap_or(0.0)
}
