#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tandem::factory::approval::Caller;
use tandem::factory::broker::gate::{AuthType, ProviderTermsRow, consent_provider_with_terms};
use tandem::factory::broker::terms::{TERMS_RECORD_PATH, TermsVerdict};
use tandem::factory::checks::set_checks;
use tandem::factory::executor::{BrokerExecutor, Executor, StageJob, StageOutcome};
use tandem::factory::model::{Actor, ActorKind};
use tandem::factory::runner::drive_run_step_with_gh;
use tandem::factory::settings;
use tandem::factory::store::FactoryStore;

use super::TempDir;

pub const FIXED_TODAY: &str = "2026-11-19";

pub const TEST_TERMS: &[ProviderTermsRow] = &[ProviderTermsRow {
    provider: "claude",
    auth_type: AuthType::ApiKey,
    verdict: TermsVerdict::AllowedWithConditions,
    conditions: "Test terms table for hermetic factory E2E suite.",
    verified_on: "2026-11-16",
    reverify_by: "2027-02-14",
    record_ref: TERMS_RECORD_PATH,
}];

pub fn user_caller<'a>() -> Caller<'a> {
    Caller::cli(true, &|_| None, &|_| None)
}

pub fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "user".to_string(),
    }
}

pub fn orch_actor() -> Actor {
    Actor {
        kind: ActorKind::Orchestrator,
        id: "orch-1".to_string(),
    }
}

pub fn sys_actor() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    }
}

pub fn stub_provider_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("test")
        .join("fixtures")
        .join("factory")
        .join("stub-provider")
        .canonicalize()
        .expect("stub-provider directory exists")
}

pub struct FixtureExecutor {
    pub broker: BrokerExecutor,
    pub parent_env: BTreeMap<String, String>,
    pub terms: &'static [ProviderTermsRow],
    pub today: String,
}

impl Executor for FixtureExecutor {
    fn run(&mut self, job: &StageJob) -> StageOutcome {
        self.broker
            .run_with_env_and_gate(job, self.parent_env.clone(), self.terms, &self.today)
    }
}

pub struct FactoryFixture {
    _root: TempDir,
    pub workspace: PathBuf,
    pub bare_remote: PathBuf,
    pub stub_dir: PathBuf,
    pub claude_config_dir: PathBuf,
    pub gh_log_path: PathBuf,
    pub gh_bin: PathBuf,
    pub store: FactoryStore,
}

impl FactoryFixture {
    pub fn new(scenario: &str) -> Self {
        let root = TempDir::new();
        let workspace = root.path().join("workspace");
        let bare_remote = root.path().join("origin.git");
        let claude_config_dir = root.path().join("claude-config");
        let gh_log_path = root.path().join("gh-calls.log");
        let bin_dir = root.path().join("bin");
        let gh_bin = bin_dir.join("gh");
        let stub_dir = stub_provider_dir();

        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&bare_remote).unwrap();
        fs::create_dir_all(&claude_config_dir).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();
        fs::write(claude_config_dir.join("stub-scenario"), scenario).unwrap();

        let gh_stub = stub_dir.join("gh");
        let gh_wrapper = format!(
            "#!/bin/sh\nSTUB_GH_LOG=\"{}\" exec \"{}\" \"$@\"\n",
            gh_log_path.display(),
            gh_stub.display()
        );
        fs::write(&gh_bin, gh_wrapper).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&gh_bin, fs::Permissions::from_mode(0o755)).unwrap();
        }

        init_bare_remote(&bare_remote);
        init_target_repo(&workspace, &bare_remote);

        fs::create_dir_all(workspace.join(".agent-workspace/coordination/queries")).unwrap();
        fs::create_dir_all(workspace.join("specs")).unwrap();
        fs::write(workspace.join(".agent-workspace/factory-mode"), "1\n").unwrap();

        let store = FactoryStore::new(&workspace);
        store.ensure().unwrap();

        let mut cfg = store.settings().unwrap();
        cfg.approval_grace_secs = 0;
        settings::save(&store, &cfg).unwrap();

        let caller = user_caller();
        set_checks(
            &store,
            "workspace",
            vec![vec!["sh".to_string(), "test/run.sh".to_string()]],
            vec![],
            Some(30),
            vec![],
            "workspace",
            "user",
            &caller,
        )
        .unwrap();

        consent_provider_with_terms(
            &store,
            "claude",
            AuthType::ApiKey,
            "claude",
            "user",
            &caller,
            FIXED_TODAY,
            TEST_TERMS,
        )
        .unwrap();

        Self {
            _root: root,
            workspace,
            bare_remote,
            stub_dir,
            claude_config_dir,
            gh_log_path,
            gh_bin,
            store,
        }
    }

    pub fn set_scenario(&self, scenario: &str) {
        fs::write(self.claude_config_dir.join("stub-scenario"), scenario).unwrap();
    }

    pub fn executor(&self) -> FixtureExecutor {
        let mut parent_env: BTreeMap<String, String> = std::env::vars().collect();
        let orig_path = parent_env.get("PATH").cloned().unwrap_or_default();
        let new_path = if orig_path.is_empty() {
            self.stub_dir.display().to_string()
        } else {
            format!("{}:{orig_path}", self.stub_dir.display())
        };
        parent_env.insert("PATH".to_string(), new_path);
        parent_env.insert(
            "CLAUDE_CONFIG_DIR".to_string(),
            self.claude_config_dir.display().to_string(),
        );
        FixtureExecutor {
            broker: BrokerExecutor {
                workspace: self.workspace.clone(),
            },
            parent_env,
            terms: TEST_TERMS,
            today: FIXED_TODAY.to_string(),
        }
    }

    pub fn drive_step(&self, run_id: &str) -> Result<(), tandem::factory::runner::RunnerError> {
        let mut exec = self.executor();
        drive_run_step_with_gh(
            &self.store,
            &self.workspace,
            run_id,
            &mut exec,
            &self.gh_bin,
        )
    }
}

fn run_git_cmd(dir: &Path, args: &[&str]) {
    let out = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_bare_remote(dir: &Path) {
    run_git_cmd(dir, &["init", "--bare", "-b", "main"]);
}

fn init_target_repo(dir: &Path, bare_remote: &Path) {
    run_git_cmd(dir, &["init", "-b", "main"]);
    run_git_cmd(dir, &["config", "user.email", "test@example.com"]);
    run_git_cmd(dir, &["config", "user.name", "Test User"]);

    fs::write(dir.join("README.md"), "# Team Activity\n").unwrap();
    fs::write(dir.join(".gitignore"), ".agent-workspace/\nworktrees/\n").unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("test")).unwrap();
    fs::write(
        dir.join("src/app.sh"),
        "#!/bin/sh\necho \"activity-core-ok\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("test/run.sh"),
        "#!/bin/sh\nset -eu\nout=\"$(sh src/app.sh)\"\n[ \"$out\" = \"activity-core-ok\" ]\nfor t in test/*_test.sh; do\n  [ -e \"$t\" ] || continue\n  sh \"$t\"\ndone\n",
    )
    .unwrap();

    run_git_cmd(dir, &["add", "."]);
    run_git_cmd(dir, &["commit", "-m", "initial commit"]);
    run_git_cmd(
        dir,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widget.git",
        ],
    );
    run_git_cmd(
        dir,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            &bare_remote.display().to_string(),
        ],
    );
    run_git_cmd(dir, &["push", "-u", "origin", "main"]);
}
