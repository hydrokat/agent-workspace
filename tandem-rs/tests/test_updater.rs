use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use tandem::updater::{UpdateRunner, WhichFn, plan, run_update};

struct Recorder {
    calls: Rc<RefCell<Vec<Vec<String>>>>,
    codes: HashMap<String, i32>,
}

impl Recorder {
    fn new(codes: HashMap<String, i32>) -> Self {
        Self {
            calls: Rc::new(RefCell::new(Vec::new())),
            codes,
        }
    }
}

fn have<'a>(names: &'a [&'a str]) -> impl Fn(&str) -> Option<PathBuf> + use<'a> {
    move |n: &str| {
        if names.contains(&n) {
            Some(PathBuf::from(format!("/bin/{n}")))
        } else {
            None
        }
    }
}

fn installed_workspace(tmp_path: &Path) -> PathBuf {
    let ws = tmp_path.join("ws");
    let tandem_dir = ws.join(".agent-workspace").join("tandem");
    fs::create_dir_all(&tandem_dir).unwrap();
    ws
}

#[test]
fn test_plan_mentions_the_ai_cli_update() {
    let dir = std::env::temp_dir();
    let text = plan(&dir, "o/r", Some(true)).join("\n");
    assert!(text.contains("AI CLIs"));
    assert!(text.contains("claude update"));
}

#[test]
fn test_runs_update_then_reload_in_order() {
    let dir = std::env::temp_dir().join("tandem_test_updater_order");
    let ws = installed_workspace(&dir);

    let state_dir = dir.join("state");
    let bin_dir = state_dir.join("bin").join("current").join("tandem");
    fs::create_dir_all(&bin_dir).unwrap();
    let fake_bin = bin_dir.join("tandem");
    fs::write(&fake_bin, b"#!/bin/sh\n").unwrap();

    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    let calls_clone = calls.clone();

    let fake_runner: Box<UpdateRunner> = Box::new(move |argv, _cwd, _timeout| {
        calls_clone
            .borrow_mut()
            .push(argv.iter().map(|s| s.to_string()).collect());
        if argv.contains(&"--fetch-runtime") {
            (0, format!("TANDEM_SELF={}\n", fake_bin.display()))
        } else {
            (0, String::new())
        }
    });

    let fake_quiet: Box<UpdateRunner> = Box::new(move |argv, _cwd, _timeout| {
        if argv.last() == Some(&"--version") {
            (0, "tandem 9.9.9".to_string())
        } else {
            (0, String::new())
        }
    });

    let reloads: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    let reloads_clone = reloads.clone();
    let reload = move || {
        reloads_clone.borrow_mut().push(1);
        true
    };

    let which_fn: Box<WhichFn> = Box::new(have(&["npx"]));

    let result = run_update(
        &ws,
        "o/r",
        reload,
        Some(&*fake_runner),
        Some(&*fake_quiet),
        Some(&*which_fn),
        Some(true),
        Some(&state_dir),
    );

    assert!(result.ok);
    assert!(result.reloaded);
    assert_eq!(*reloads.borrow(), vec![1]);
    assert!(result.message.contains("9.9.9"));
    assert_eq!(calls.borrow()[0][0], "npx");
    assert!(calls.borrow()[1][1].contains("--fetch-runtime"));
}

#[test]
fn test_failed_workspace_update_restarts_nothing() {
    let dir = std::env::temp_dir().join("tandem_test_updater_failed_ws");
    let ws = installed_workspace(&dir);

    let reloads: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    let reloads_clone = reloads.clone();
    let reload = move || {
        reloads_clone.borrow_mut().push(1);
        true
    };

    let rec = Recorder::new(HashMap::from([("npx".to_string(), 1)]));
    let calls = rec.calls.clone();
    let codes = rec.codes.clone();
    let runner: Box<UpdateRunner> = Box::new(move |argv, _cwd, _timeout| {
        calls
            .borrow_mut()
            .push(argv.iter().map(|s| s.to_string()).collect());
        let code = codes.get(argv[0]).copied().unwrap_or(0);
        (code, String::new())
    });

    let which_fn: Box<WhichFn> = Box::new(have(&["npx"]));

    let result = run_update(
        &ws,
        "o/r",
        reload,
        Some(&*runner),
        None,
        Some(&*which_fn),
        Some(true),
        None,
    );

    assert!(!result.ok);
    assert!(result.message.contains("Nothing was restarted"));
    assert!(reloads.borrow().is_empty());
}

#[test]
fn test_missing_npx_is_explained() {
    let dir = std::env::temp_dir();
    let which_fn: Box<WhichFn> = Box::new(have(&[]));

    let result = run_update(
        &dir,
        "o/r",
        || true,
        None,
        None,
        Some(&*which_fn),
        Some(true),
        None,
    );

    assert!(!result.ok);
    assert!(result.message.contains("Node.js"));
}

#[test]
fn test_checkout_copy_only_updates_the_workspace() {
    let dir = std::env::temp_dir().join("tandem_test_updater_checkout");
    let ws = dir.join("ws");
    fs::create_dir_all(&ws).unwrap();

    let calls: Rc<RefCell<Vec<Vec<String>>>> = Rc::new(RefCell::new(Vec::new()));
    let calls_clone = calls.clone();
    let runner: Box<UpdateRunner> = Box::new(move |argv, _cwd, _timeout| {
        calls_clone
            .borrow_mut()
            .push(argv.iter().map(|s| s.to_string()).collect());
        (0, String::new())
    });

    let reloads: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    let reloads_clone = reloads.clone();
    let reload = move || {
        reloads_clone.borrow_mut().push(1);
        true
    };
    let which_fn: Box<WhichFn> = Box::new(have(&["npx"]));

    let result = run_update(
        &ws,
        "o/r",
        reload,
        Some(&*runner),
        None,
        Some(&*which_fn),
        Some(false),
        None,
    );

    assert!(result.ok);
    assert_eq!(calls.borrow().len(), 1);
    assert_eq!(calls.borrow()[0][0], "npx");
    assert!(reloads.borrow().is_empty());
    assert!(result.message.contains("checkout"));
}

#[test]
fn test_min_versions_change_is_called_out() {
    let dir = std::env::temp_dir().join("tandem_test_updater_min_ver");
    let ws = installed_workspace(&dir);
    let min_file = ws
        .join(".agent-workspace")
        .join("tandem")
        .join("min-versions.env");
    fs::write(&min_file, b"TANDEM_MIN_TMUX=3.3\n").unwrap();

    let state_dir = dir.join("state");
    let bin_dir = state_dir.join("bin").join("current").join("tandem");
    fs::create_dir_all(&bin_dir).unwrap();
    let fake_bin = bin_dir.join("tandem");
    fs::write(&fake_bin, b"#!/bin/sh\n").unwrap();

    let min_clone = min_file.clone();
    let runner: Box<UpdateRunner> = Box::new(move |argv, _cwd, _timeout| {
        if argv[0] == "npx" {
            fs::write(&min_clone, b"TANDEM_MIN_TMUX=3.4\n").unwrap();
        }
        if argv.contains(&"--fetch-runtime") {
            (0, format!("TANDEM_SELF={}\n", fake_bin.display()))
        } else {
            (0, String::new())
        }
    });

    let quiet: Box<UpdateRunner> =
        Box::new(move |_argv, _cwd, _timeout| (0, "tandem 1.5.0".to_string()));
    let which_fn: Box<WhichFn> = Box::new(have(&["npx"]));

    let result = run_update(
        &ws,
        "o/r",
        || true,
        Some(&*runner),
        Some(&*quiet),
        Some(&*which_fn),
        Some(true),
        Some(&state_dir),
    );

    assert!(result.ok);
    assert!(
        result
            .message
            .contains("Minimum dependency versions changed")
    );
}

#[test]
fn test_frozen_plan_mentions_fetch_runtime() {
    let dir = std::env::temp_dir();
    let text = plan(&dir, "o/r", Some(true)).join("\n");
    assert!(text.contains("run.sh --fetch-runtime"));
    assert!(!text.contains("uv sync"));
}

#[test]
fn test_frozen_updater_fetch_failure() {
    let dir = std::env::temp_dir().join("tandem_test_updater_fetch_fail");
    let ws = installed_workspace(&dir);

    let runner: Box<UpdateRunner> = Box::new(|argv, _cwd, _timeout| {
        if argv.contains(&"--fetch-runtime") {
            (4, "checksum mismatch".to_string())
        } else {
            (0, String::new())
        }
    });

    let reloads: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    let reloads_clone = reloads.clone();
    let reload = move || {
        reloads_clone.borrow_mut().push(1);
        true
    };
    let which_fn: Box<WhichFn> = Box::new(have(&["npx"]));

    let result = run_update(
        &ws,
        "o/r",
        reload,
        Some(&*runner),
        None,
        Some(&*which_fn),
        Some(true),
        None,
    );

    assert!(!result.ok);
    assert!(reloads.borrow().is_empty());
    assert!(result.message.contains("could not be fetched"));
    assert!(result.message.contains("checksum mismatch"));
}

#[test]
fn test_frozen_updater_missing_self_reported() {
    let dir = std::env::temp_dir().join("tandem_test_updater_missing_self");
    let ws = installed_workspace(&dir);

    let runner: Box<UpdateRunner> = Box::new(|argv, _cwd, _timeout| {
        if argv.contains(&"--fetch-runtime") {
            (0, "some output without self".to_string())
        } else {
            (0, String::new())
        }
    });

    let which_fn: Box<WhichFn> = Box::new(have(&["npx"]));

    let result = run_update(
        &ws,
        "o/r",
        || true,
        Some(&*runner),
        None,
        Some(&*which_fn),
        Some(true),
        None,
    );

    assert!(!result.ok);
    assert!(result.message.contains("TANDEM_SELF not reported"));
}

#[test]
fn test_frozen_updater_invalid_path() {
    let dir = std::env::temp_dir().join("tandem_test_updater_invalid_path");
    let ws = installed_workspace(&dir);

    let runner: Box<UpdateRunner> = Box::new(|argv, _cwd, _timeout| {
        if argv.contains(&"--fetch-runtime") {
            (0, "TANDEM_SELF=/not/in/state/dir/bin\n".to_string())
        } else {
            (0, String::new())
        }
    });

    let which_fn: Box<WhichFn> = Box::new(have(&["npx"]));

    let result = run_update(
        &ws,
        "o/r",
        || true,
        Some(&*runner),
        None,
        Some(&*which_fn),
        Some(true),
        None,
    );

    assert!(!result.ok);
    assert!(result.message.contains("invalid binary path"));
}
