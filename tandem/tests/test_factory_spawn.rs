use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tandem::factory::broker::gate::{GatePass, check};
use tandem::factory::broker::spawn::{MAX_LOG_BYTES, ProviderCommand, SpawnError, spawn_provider};
use tandem::factory::model::{FactorySettings, ProviderGate, ProviderGateState};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-spawn-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn open_claude_pass() -> GatePass {
    let settings = FactorySettings {
        provider_gates: vec![ProviderGate {
            provider: "claude".into(),
            state: ProviderGateState::Open,
            auth_type: Some("api-key".into()),
            verified_on: Some("2026-11-16".into()),
            terms_record: Some(
                "knowledgebase/architecture-decisions/factory-provider-terms.md".into(),
            ),
            consented_at: Some(1000.0),
            consented_by: Some("user".into()),
        }],
        ..Default::default()
    };
    check("claude", &settings, "2026-11-17").unwrap()
}

fn write_mock_cli(bin_dir: &Path, name: &str, script_body: &str) {
    fs::create_dir_all(bin_dir).unwrap();
    let path = bin_dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
    let mut perms = fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).unwrap();
}

fn base_parent_env(bin_dir: &Path) -> BTreeMap<String, String> {
    let sys_path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string());
    BTreeMap::from([
        (
            "PATH".to_string(),
            format!("{}:{sys_path}", bin_dir.display()),
        ),
        ("HOME".to_string(), "/tmp/tandem-test-home".to_string()),
        ("LANG".to_string(), "en_US.UTF-8".to_string()),
        ("LC_ALL".to_string(), "en_US.UTF-8".to_string()),
        ("TERM".to_string(), "xterm-256color".to_string()),
        (
            "CLAUDE_CONFIG_DIR".to_string(),
            "/tmp/tandem-test-claude-cfg".to_string(),
        ),
    ])
}

#[test]
fn rejects_unknown_program_and_program_mismatch_with_gate_pass() {
    let ws = temp_dir("mismatch");
    let worktree = ws.join("worktrees").join("run-1");
    fs::create_dir_all(&worktree).unwrap();
    let log_path = ws.join("job.log");

    let cmd_bad = ProviderCommand {
        program: "bash".to_string(),
        args: vec![],
        workspace_root: ws.clone(),
        cwd: worktree.clone(),
        log_path: log_path.clone(),
        stdin_prompt: None,
        factory_env: BTreeMap::new(),
        parent_env: BTreeMap::new(),
    };
    let err = spawn_provider(open_claude_pass(), cmd_bad).unwrap_err();
    assert!(matches!(err, SpawnError::UnknownProgram { .. }));

    let cmd_mismatch = ProviderCommand {
        program: "agy".to_string(),
        args: vec![],
        workspace_root: ws.clone(),
        cwd: worktree,
        log_path,
        stdin_prompt: None,
        factory_env: BTreeMap::new(),
        parent_env: BTreeMap::new(),
    };
    let err = spawn_provider(open_claude_pass(), cmd_mismatch).unwrap_err();
    assert!(matches!(err, SpawnError::ProgramMismatch { .. }));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn rejects_cwd_outside_worktrees_and_symlinks() {
    let ws = temp_dir("cwd");
    let worktrees = ws.join("worktrees");
    let valid_wt = worktrees.join("run-1");
    let outside = temp_dir("outside");
    fs::create_dir_all(&valid_wt).unwrap();
    let bin_dir = ws.join("bin");
    write_mock_cli(&bin_dir, "claude", "exit 0");

    let evil_link = worktrees.join("evil-link");
    symlink(&outside, &evil_link).unwrap();

    for bad_cwd in [
        ws.clone(),
        outside.clone(),
        worktrees.clone(),
        evil_link,
        valid_wt.join("..").join(".."),
    ] {
        let cmd = ProviderCommand {
            program: "claude".to_string(),
            args: vec![],
            workspace_root: ws.clone(),
            cwd: bad_cwd,
            log_path: ws.join("job.log"),
            stdin_prompt: None,
            factory_env: BTreeMap::new(),
            parent_env: base_parent_env(&bin_dir),
        };
        let err = spawn_provider(open_claude_pass(), cmd).unwrap_err();
        assert!(
            matches!(err, SpawnError::InvalidWorktree { .. }),
            "expected InvalidWorktree, got {err:?}"
        );
    }

    let ok_cmd = ProviderCommand {
        program: "claude".to_string(),
        args: vec![],
        workspace_root: ws.clone(),
        cwd: valid_wt,
        log_path: ws.join("ok.log"),
        stdin_prompt: None,
        factory_env: BTreeMap::new(),
        parent_env: base_parent_env(&bin_dir),
    };
    let mut job = spawn_provider(open_claude_pass(), ok_cmd).unwrap();
    let status = job.wait(Duration::from_secs(5)).unwrap();
    assert!(status.success());

    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&outside);
}

#[test]
fn environment_is_strictly_allow_listed_and_drops_secrets() {
    let ws = temp_dir("env");
    let worktree = ws.join("worktrees").join("run-1");
    fs::create_dir_all(&worktree).unwrap();
    let bin_dir = ws.join("bin");
    write_mock_cli(&bin_dir, "claude", "env");

    let mut parent_env = base_parent_env(&bin_dir);
    parent_env.insert("ANTHROPIC_API_KEY".into(), "sk-ant-secret".into());
    parent_env.insert("OPENAI_API_KEY".into(), "sk-openai-secret".into());
    parent_env.insert("CLAUDE_CODE_OAUTH_TOKEN".into(), "oauth-secret".into());
    parent_env.insert("AWS_SECRET_ACCESS_KEY".into(), "aws-secret".into());
    parent_env.insert("CUSTOM_SECRET".into(), "should-not-leak".into());

    let factory_env = BTreeMap::from([
        ("TANDEM_SESSION_ID".into(), "impl-1".into()),
        ("TANDEM_ROLE".into(), "implementer".into()),
        ("TANDEM_RUN_ID".into(), "run-1".into()),
        ("EVIL_INJECTED".into(), "nope".into()),
        ("TANDEM_SECRET_TOKEN".into(), "nope-token".into()),
    ]);

    let log_path = ws.join("env.log");
    let cmd = ProviderCommand {
        program: "claude".to_string(),
        args: vec![],
        workspace_root: ws.clone(),
        cwd: worktree,
        log_path: log_path.clone(),
        stdin_prompt: None,
        factory_env,
        parent_env,
    };

    let mut job = spawn_provider(open_claude_pass(), cmd).unwrap();
    let status = job.wait(Duration::from_secs(5)).unwrap();
    assert!(status.success());

    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("HOME=/tmp/tandem-test-home"));
    assert!(log.contains("LANG=en_US.UTF-8"));
    assert!(log.contains("LC_ALL=en_US.UTF-8"));
    assert!(log.contains("TERM=xterm-256color"));
    assert!(log.contains("CLAUDE_CONFIG_DIR=/tmp/tandem-test-claude-cfg"));
    assert!(log.contains("TANDEM_SESSION_ID=impl-1"));
    assert!(log.contains("TANDEM_ROLE=implementer"));
    assert!(log.contains("TANDEM_RUN_ID=run-1"));

    for forbidden in [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "AWS_SECRET_ACCESS_KEY",
        "CUSTOM_SECRET",
        "EVIL_INJECTED",
        "TANDEM_SECRET_TOKEN",
    ] {
        assert!(!log.contains(forbidden), "secret {forbidden} leaked: {log}");
    }

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn pipes_stdin_prompt_and_passes_argv_without_shell() {
    let ws = temp_dir("stdin");
    let worktree = ws.join("worktrees").join("run-1");
    fs::create_dir_all(&worktree).unwrap();
    let bin_dir = ws.join("bin");
    write_mock_cli(
        &bin_dir,
        "claude",
        "printf 'ARG1=%s\\nARG2=%s\\n' \"$1\" \"$2\"\ncat",
    );

    let log_path = ws.join("stdin.log");
    let cmd = ProviderCommand {
        program: "claude".to_string(),
        args: vec!["-p".into(), "$(echo pwned); id".into()],
        workspace_root: ws.clone(),
        cwd: worktree,
        log_path: log_path.clone(),
        stdin_prompt: Some("prompt line 1\nprompt line 2\n".into()),
        factory_env: BTreeMap::new(),
        parent_env: base_parent_env(&bin_dir),
    };

    let mut job = spawn_provider(open_claude_pass(), cmd).unwrap();
    let status = job.wait(Duration::from_secs(5)).unwrap();
    assert!(status.success());

    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("ARG1=-p\n"));
    assert!(log.contains("ARG2=$(echo pwned); id\n"));
    assert!(log.contains("prompt line 1\nprompt line 2\n"));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn caps_log_output_at_16_mib_and_stops_job() {
    let ws = temp_dir("logcap");
    let worktree = ws.join("worktrees").join("run-1");
    fs::create_dir_all(&worktree).unwrap();
    let bin_dir = ws.join("bin");
    write_mock_cli(
        &bin_dir,
        "claude",
        "dd if=/dev/zero bs=65536 count=300 2>/dev/null | tr '\\0' 'A'",
    );

    let log_path = ws.join("big.log");
    let cmd = ProviderCommand {
        program: "claude".to_string(),
        args: vec![],
        workspace_root: ws.clone(),
        cwd: worktree,
        log_path: log_path.clone(),
        stdin_prompt: None,
        factory_env: BTreeMap::new(),
        parent_env: base_parent_env(&bin_dir),
    };

    let mut job = spawn_provider(open_claude_pass(), cmd).unwrap();
    let err = job.wait(Duration::from_secs(15)).unwrap_err();
    assert!(matches!(err, SpawnError::LogCapExceeded { .. }));

    let size = fs::metadata(&log_path).unwrap().len() as usize;
    assert!(
        size <= MAX_LOG_BYTES,
        "log size {size} exceeded {MAX_LOG_BYTES}"
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn wait_times_out_and_stop_closes_stdin_waits_grace_then_kills() {
    let ws = temp_dir("stop");
    let worktree = ws.join("worktrees").join("run-1");
    fs::create_dir_all(&worktree).unwrap();
    let bin_dir = ws.join("bin");

    // Case 1: exits cleanly when stdin closes
    write_mock_cli(&bin_dir, "claude", "cat >/dev/null\nexit 0");
    let cmd_graceful = ProviderCommand {
        program: "claude".to_string(),
        args: vec![],
        workspace_root: ws.clone(),
        cwd: worktree.clone(),
        log_path: ws.join("graceful.log"),
        stdin_prompt: None,
        factory_env: BTreeMap::new(),
        parent_env: base_parent_env(&bin_dir),
    };
    let mut job = spawn_provider(open_claude_pass(), cmd_graceful).unwrap();
    let err = job.wait(Duration::from_millis(120)).unwrap_err();
    assert!(matches!(err, SpawnError::TimedOut { .. }));

    let status = job.stop(Duration::from_secs(2)).unwrap();
    assert!(status.success());

    // Case 2: ignores stdin and must be killed after grace
    write_mock_cli(&bin_dir, "claude", "sleep 30");
    let cmd_kill = ProviderCommand {
        program: "claude".to_string(),
        args: vec![],
        workspace_root: ws.clone(),
        cwd: worktree,
        log_path: ws.join("kill.log"),
        stdin_prompt: None,
        factory_env: BTreeMap::new(),
        parent_env: base_parent_env(&bin_dir),
    };
    let mut job2 = spawn_provider(open_claude_pass(), cmd_kill).unwrap();
    let start = Instant::now();
    let status2 = job2.stop(Duration::from_millis(150)).unwrap();
    let elapsed = start.elapsed();
    assert!(!status2.success());
    assert!(elapsed >= Duration::from_millis(120));

    let _ = fs::remove_dir_all(&ws);
}
