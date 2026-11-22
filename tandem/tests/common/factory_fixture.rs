#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as StdCommand, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tandem::factory::approval::Caller;
use tandem::factory::broker::gate::{AuthType, ProviderTermsRow, consent_provider_with_terms};
use tandem::factory::broker::terms::{TERMS_RECORD_PATH, TermsVerdict};
use tandem::factory::checks::set_checks;
use tandem::factory::executor::{BrokerExecutor, Executor, StageJob, StageOutcome};
use tandem::factory::login::LoginProbe;
use tandem::factory::model::{Actor, ActorKind, Run};
use tandem::factory::runner::{RunnerError, drive_run_step_with_gh, resume_paused_run_with_probe};
use tandem::factory::settings;
use tandem::factory::store::FactoryStore;
use tandem::tmux::SESSION;
use tandem::tmux::client::{Result as TmuxCmdResult, TmuxClient, TmuxRunner};

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

pub struct StubEnvLoginProbe {
    pub env: BTreeMap<String, String>,
    pub signed_out: bool,
    pub stub_only_dir: Option<PathBuf>,
}

impl LoginProbe for StubEnvLoginProbe {
    fn which(&self, bin: &str) -> Option<PathBuf> {
        if let Some(ref only_dir) = self.stub_only_dir {
            let candidate = only_dir.join(bin);
            return candidate.is_file().then_some(candidate);
        }
        let path_val = self.env.get("PATH")?;
        for dir in std::env::split_paths(path_val) {
            let candidate = dir.join(bin);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    fn version(&self, cli: &str) -> String {
        format!("{cli} stub-1.0")
    }

    fn run(&self, argv: &[&str]) -> (i32, String, String) {
        if self.signed_out {
            return (1, String::new(), "not signed in".to_string());
        }
        let Some((prog, args)) = argv.split_first() else {
            return (1, String::new(), "empty argv".to_string());
        };
        let Some(resolved) = self.which(prog) else {
            return (127, String::new(), format!("{prog} not found"));
        };
        let mut cmd = StdCommand::new(resolved);
        cmd.args(args)
            .env_clear()
            .envs(&self.env)
            .stdin(Stdio::null());
        match cmd.output() {
            Ok(out) => (
                out.status.code().unwrap_or(1),
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ),
            Err(e) => (1, String::new(), e.to_string()),
        }
    }
}

struct StubPane {
    id: String,
    window: String,
    kind: String,
    role: String,
    index: String,
    label: String,
    run: String,
    job: String,
    stage: String,
    cli: String,
    model: String,
    child: Option<Child>,
    dead: bool,
    dead_status: i32,
}

#[derive(Default)]
pub struct StubWorkerTmuxRunner {
    next_id: AtomicUsize,
    panes: Mutex<HashMap<String, StubPane>>,
}

impl StubWorkerTmuxRunner {
    pub fn new() -> Self {
        Self {
            next_id: AtomicUsize::new(100),
            panes: Mutex::new(HashMap::new()),
        }
    }
}

impl Drop for StubWorkerTmuxRunner {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.panes.lock() {
            for pane in guard.values_mut() {
                if let Some(ref mut child) = pane.child {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

impl TmuxRunner for StubWorkerTmuxRunner {
    fn run(&self, argv: &[String]) -> TmuxCmdResult {
        let mut rest = if argv.len() >= 3 && argv[0] == "tmux" && argv[1] == "-L" {
            if argv.get(3).map(String::as_str) == Some("-f") {
                &argv[5..]
            } else {
                &argv[3..]
            }
        } else {
            argv
        };
        if rest.is_empty() {
            return TmuxCmdResult {
                code: 0,
                out: String::new(),
                err: String::new(),
            };
        }
        match rest[0].as_str() {
            "new-window" => {
                rest = &rest[1..];
                let mut window = String::new();
                let mut cwd = std::env::temp_dir();
                let mut i = 0;
                while i < rest.len() {
                    match rest[i].as_str() {
                        "-P" => i += 1,
                        "-F" | "-t" => i += 2,
                        "-n" => {
                            window = rest.get(i + 1).cloned().unwrap_or_default();
                            i += 2;
                        }
                        "-c" => {
                            if let Some(c) = rest.get(i + 1) {
                                cwd = PathBuf::from(c);
                            }
                            i += 2;
                        }
                        _ => break,
                    }
                }
                let cmd_tokens = &rest[i..];
                let mut child_env: BTreeMap<String, String> = BTreeMap::new();
                let mut cmd_idx = 0;
                if cmd_tokens.first().map(String::as_str) == Some("env") {
                    cmd_idx = 1;
                    if cmd_tokens.get(cmd_idx).map(String::as_str) == Some("-i") {
                        cmd_idx += 1;
                    }
                    while cmd_idx < cmd_tokens.len() && cmd_tokens[cmd_idx].contains('=') {
                        if let Some((k, v)) = cmd_tokens[cmd_idx].split_once('=') {
                            child_env.insert(k.to_string(), v.to_string());
                        }
                        cmd_idx += 1;
                    }
                }
                let Some(prog) = cmd_tokens.get(cmd_idx) else {
                    return TmuxCmdResult {
                        code: 1,
                        out: String::new(),
                        err: "missing worker command".to_string(),
                    };
                };
                let args = &cmd_tokens[cmd_idx + 1..];
                let resolved = if prog.contains('/') {
                    PathBuf::from(prog)
                } else {
                    child_env
                        .get("PATH")
                        .and_then(|p| {
                            std::env::split_paths(p)
                                .map(|d| d.join(prog))
                                .find(|cand| cand.is_file())
                        })
                        .unwrap_or_else(|| PathBuf::from(prog))
                };
                let child = match StdCommand::new(resolved)
                    .args(args)
                    .current_dir(&cwd)
                    .env_clear()
                    .envs(&child_env)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(c) => c,
                    Err(e) => {
                        return TmuxCmdResult {
                            code: 1,
                            out: String::new(),
                            err: format!("spawn failed: {e}"),
                        };
                    }
                };
                let id_num = self.next_id.fetch_add(1, Ordering::Relaxed);
                let pane_id = format!("%{id_num}");
                self.panes.lock().unwrap().insert(
                    pane_id.clone(),
                    StubPane {
                        id: pane_id.clone(),
                        window,
                        kind: String::new(),
                        role: String::new(),
                        index: "0".to_string(),
                        label: String::new(),
                        run: String::new(),
                        job: String::new(),
                        stage: String::new(),
                        cli: String::new(),
                        model: String::new(),
                        child: Some(child),
                        dead: false,
                        dead_status: 0,
                    },
                );
                TmuxCmdResult {
                    code: 0,
                    out: format!("{pane_id}\n"),
                    err: String::new(),
                }
            }
            "set-option" => {
                let mut pane_id = String::new();
                let mut i = 1;
                while i < rest.len() {
                    if rest[i] == "-p" {
                        i += 1;
                    } else if rest[i] == "-t" {
                        pane_id = rest.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    } else {
                        break;
                    }
                }
                if let (Some(key), Some(val)) = (rest.get(i), rest.get(i + 1))
                    && let Some(pane) = self.panes.lock().unwrap().get_mut(&pane_id)
                {
                    match key.as_str() {
                        "@tandem_kind" => pane.kind = val.clone(),
                        "@tandem_role" => pane.role = val.clone(),
                        "@tandem_index" => pane.index = val.clone(),
                        "@tandem_label" => pane.label = val.clone(),
                        "@tandem_run" => pane.run = val.clone(),
                        "@tandem_job" => pane.job = val.clone(),
                        "@tandem_stage" => pane.stage = val.clone(),
                        "@tandem_cli" => pane.cli = val.clone(),
                        "@tandem_model" => pane.model = val.clone(),
                        _ => {}
                    }
                }
                TmuxCmdResult {
                    code: 0,
                    out: String::new(),
                    err: String::new(),
                }
            }
            "list-panes" => {
                let mut guard = self.panes.lock().unwrap();
                let mut lines = Vec::new();
                for pane in guard.values_mut() {
                    if !pane.dead
                        && let Some(ref mut child) = pane.child
                        && let Ok(Some(status)) = child.try_wait()
                    {
                        pane.dead = true;
                        pane.dead_status = status.code().unwrap_or(1);
                    }
                    let dead_flag = if pane.dead { "1" } else { "0" };
                    lines.push(
                        [
                            pane.id.as_str(),
                            pane.window.as_str(),
                            pane.kind.as_str(),
                            pane.role.as_str(),
                            pane.index.as_str(),
                            pane.label.as_str(),
                            dead_flag,
                            &pane.dead_status.to_string(),
                            "",
                            "",
                            "",
                            "",
                            "",
                            pane.run.as_str(),
                            pane.job.as_str(),
                            pane.stage.as_str(),
                            pane.cli.as_str(),
                            pane.model.as_str(),
                        ]
                        .join("\t"),
                    );
                }
                TmuxCmdResult {
                    code: 0,
                    out: lines.join("\n"),
                    err: String::new(),
                }
            }
            "kill-pane" => {
                let mut pane_id = String::new();
                for w in rest.windows(2) {
                    if w[0] == "-t" {
                        pane_id = w[1].clone();
                    }
                }
                if let Some(mut pane) = self.panes.lock().unwrap().remove(&pane_id)
                    && let Some(ref mut child) = pane.child
                {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                TmuxCmdResult {
                    code: 0,
                    out: String::new(),
                    err: String::new(),
                }
            }
            other => TmuxCmdResult {
                code: 1,
                out: String::new(),
                err: format!("unsupported stub tmux command: {other}"),
            },
        }
    }
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

    fn ready_clis(&self) -> Vec<tandem::factory::executor::ReadyCli> {
        self.broker.ready_clis()
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

    pub fn acknowledge_risky_clis(&self, probe: &dyn LoginProbe) {
        use tandem::factory::auth_risk::{
            AuthRisk, acknowledge_with_terms, assess, detected_method, method_name,
        };
        use tandem::factory::login::check_status;
        let caller = user_caller();
        for cli in ["claude", "agy", "opencode"] {
            let item = check_status(cli, probe);
            if !(item.installed && item.signed_in) {
                continue;
            }
            let detected = detected_method(&item);
            if matches!(
                assess(cli, detected, FIXED_TODAY, TEST_TERMS, &[]),
                AuthRisk::Risky { .. }
            ) {
                acknowledge_with_terms(
                    &self.store,
                    cli,
                    detected,
                    method_name(detected),
                    cli,
                    "user",
                    &caller,
                    TEST_TERMS,
                )
                .unwrap();
            }
        }
    }

    pub fn set_scenario(&self, scenario: &str) {
        fs::write(self.claude_config_dir.join("stub-scenario"), scenario).unwrap();
    }

    pub fn env_map(&self) -> BTreeMap<String, String> {
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
        parent_env
    }

    pub fn login_probe(&self) -> StubEnvLoginProbe {
        StubEnvLoginProbe {
            env: self.env_map(),
            signed_out: false,
            stub_only_dir: Some(self.stub_dir.clone()),
        }
    }

    pub fn signed_out_login_probe(&self) -> StubEnvLoginProbe {
        StubEnvLoginProbe {
            env: self.env_map(),
            signed_out: true,
            stub_only_dir: Some(self.stub_dir.clone()),
        }
    }

    pub fn resume_run(&self, run_id: &str) -> Result<Run, RunnerError> {
        resume_paused_run_with_probe(&self.store, run_id, &user_actor(), &self.login_probe())
    }

    pub fn executor(&self) -> FixtureExecutor {
        let parent_env = self.env_map();
        let tmux = TmuxClient::with_runner(SESSION, None, Arc::new(StubWorkerTmuxRunner::new()));
        let probe = Arc::new(self.login_probe());
        FixtureExecutor {
            broker: BrokerExecutor::new(&self.workspace)
                .with_tmux(tmux)
                .with_login_probe(probe),
            parent_env,
            terms: TEST_TERMS,
            today: FIXED_TODAY.to_string(),
        }
    }

    pub fn drive_step(&self, run_id: &str) -> Result<(), RunnerError> {
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
