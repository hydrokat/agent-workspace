mod common;

use common::parity::load_fixture_value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tandem::quota;
use tandem::state;
use tandem::tmux::client::socket_for;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "tandem-e2e-{}-{}-{}",
            std::process::id(),
            nanos,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tandem() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tandem"))
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

fn python3() -> Option<String> {
    let out = Command::new("python3")
        .args(["-c", "print(1)"])
        .output()
        .ok()?;
    out.status.success().then(|| "python3".to_string())
}

#[test]
fn version_banner_matches_what_run_sh_expects() {
    let out = tandem().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("tandem {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn help_exits_zero_with_a_clean_environment() {
    let out = tandem()
        .arg("--help")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(out.status.success());
}

#[test]
fn session_file_written_by_python_1_5_0_resumes() {
    let fixture = load_fixture_value("state");
    let written_by_python = fixture["parse"]["good"].clone();
    let scratch = Scratch::new();
    let file = scratch.path().join("tandem-session.json");
    std::fs::write(&file, serde_json::to_string(&written_by_python).unwrap()).unwrap();

    let loaded = state::load(&file).expect("Python-written session loads");
    assert_eq!(loaded.agents.len(), 2);
    assert!(loaded.clean);
    assert_eq!(loaded.columns_percent, Some(50));
    assert_eq!(loaded.status_rows, Some(10));
    assert_eq!(loaded.active.get("planner"), Some(&1));
    assert_eq!(
        loaded.agents[0].conversation.as_deref(),
        Some("c0a80101-0000-0000-0000-000000000001")
    );

    let rewritten = scratch.path().join("rewritten.json");
    state::save(&rewritten, &loaded).unwrap();
    let again: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&rewritten).unwrap()).unwrap();
    assert_eq!(
        again, written_by_python,
        "Rust re-save keeps the Python schema"
    );
}

#[test]
fn crashed_python_session_is_described_as_unclean() {
    let fixture = load_fixture_value("state");
    let mut crashed = fixture["parse"]["good"].clone();
    crashed["clean"] = serde_json::Value::Bool(false);
    let scratch = Scratch::new();
    let file = scratch.path().join("tandem-session.json");
    std::fs::write(&file, crashed.to_string()).unwrap();

    let loaded = state::load(&file).expect("crash autosave loads");
    assert!(!loaded.clean);
    assert!(state::describe(&loaded, Some(1_700_000_100.0)).contains("did not exit cleanly"));
}

#[test]
fn unsupported_or_empty_session_files_are_rejected_not_resumed() {
    let scratch = Scratch::new();
    let file = scratch.path().join("tandem-session.json");
    std::fs::write(&file, r#"{"version": 99, "agents": [{"cli": "claude"}]}"#).unwrap();
    assert!(state::load(&file).is_none());
    std::fs::write(&file, r#"{"version": 2, "agents": []}"#).unwrap();
    assert!(state::load(&file).is_none());
    std::fs::write(&file, "not json").unwrap();
    assert!(state::load(&file).is_none());
}

#[test]
fn socket_name_matches_live_cpython_for_reattach() {
    let Some(python) = python3() else {
        eprintln!("skipping: python3 not available");
        return;
    };
    let paths = [
        "/nonexistent-e2e/workspace",
        "/nonexistent-e2e/with spaces/and-dashes_underscores",
        "/nonexistent-e2e/ünïcødé/工作区",
        "/nonexistent-e2e/deep/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z/workspace",
        "/nonexistent-e2e/x",
    ];
    for path in paths {
        let out = Command::new(&python)
            .args([
                "-c",
                "import random, sys; print('tandem-%010x' % random.Random(sys.argv[1]).getrandbits(40))",
                path,
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "python failed for {path}");
        let expected = String::from_utf8(out.stdout).unwrap().trim().to_string();
        assert_eq!(socket_for(Path::new(path)), expected, "socket for {path}");
    }
}

fn claude_statusline_sample(five_hour_reset: i64, seven_day_reset: i64) -> String {
    format!(
        r#"{{
  "session_id": "6f1d1a54-9a43-4c0f-8e0b-2a4a8f6f0c11",
  "transcript_path": "/home/dev/.claude/projects/p/6f1d1a54.jsonl",
  "cwd": "/home/dev/project",
  "model": {{"id": "claude-opus-4-7", "display_name": "Opus 4.7"}},
  "workspace": {{"current_dir": "/home/dev/project", "project_dir": "/home/dev/project"}},
  "version": "2.1.316",
  "rate_limits": {{
    "five_hour": {{"used_percentage": 23.5, "resets_at": {five_hour_reset}}},
    "seven_day": {{"used_percentage": 41.0, "resets_at": {seven_day_reset}}}
  }}
}}"#
    )
}

fn run_quota_feed(coord: &Path, home: &Path, session: &str, input: &str) -> std::process::Output {
    let mut child = tandem()
        .arg("quota-feed")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("TANDEM_COORD_DIR", coord)
        .env("TANDEM_SESSION_ID", session)
        .current_dir(home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn quota_feed_records_a_real_claude_statusline_payload() {
    let scratch = Scratch::new();
    let coord = scratch.path().join("coord");
    let home = scratch.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    let five_reset = now_secs() as i64 + 3 * 3600;
    let seven_reset = now_secs() as i64 + 6 * 86_400;
    let sample = claude_statusline_sample(five_reset, seven_reset);
    let out = run_quota_feed(&coord, &home, "planner-1", &sample);
    assert!(out.status.success());

    let found = quota::read_claude(&coord).expect("feed written and readable");
    let five = found.five_hour.clone().expect("five_hour window");
    assert_eq!(five.used_percentage, 23.5);
    assert_eq!(five.resets_at, five_reset);
    assert_eq!(
        found
            .seven_day
            .clone()
            .expect("seven_day window")
            .used_percentage,
        41.0
    );

    let line = quota::format_line(&["claude"], Some(&found), now_secs());
    assert!(
        line.contains("24%") || line.contains("23%") || line.contains("5h"),
        "line: {line}"
    );
}

#[test]
fn quota_feed_survives_garbage_and_oversized_input() {
    let scratch = Scratch::new();
    let coord = scratch.path().join("coord");
    let home = scratch.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    for input in [
        "",
        "not json",
        "{\"rate_limits\": 3}",
        &"x".repeat(quota::MAX_INPUT + 10),
    ] {
        let out = run_quota_feed(&coord, &home, "planner-2", input);
        assert!(
            out.status.success(),
            "exit ok for {:?}",
            &input[..input.len().min(12)]
        );
    }
    assert!(
        quota::read_claude(&coord).is_none(),
        "no feed from bad input"
    );
}
