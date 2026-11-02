use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if dir.is_dir()
        && let Ok(entries) = fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_rs_files(&path));
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

pub fn check_forbidden_tmux_operations(src_dir: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let forbidden = [
        "send-keys",
        "capture-pane",
        "pipe-pane",
        "paste-buffer",
        "load-buffer",
        "copy-mode",
    ];

    for file in collect_rs_files(src_dir) {
        let rel_path = file.strip_prefix(src_dir).unwrap_or(&file);
        if rel_path == Path::new("tmux/client.rs") || rel_path == Path::new("tmux\\client.rs") {
            continue;
        }

        let content = fs::read_to_string(&file).unwrap_or_default();
        for (line_no, line) in content.lines().enumerate() {
            for pattern in &forbidden {
                if line.contains(pattern) {
                    violations.push(format!(
                        "{}:{}: forbidden tmux operation '{}'",
                        rel_path.display(),
                        line_no + 1,
                        pattern
                    ));
                }
            }
        }
    }

    violations
}

pub fn check_own_pane_helpers(src_dir: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let patterns = [".send_keys_own(", ".capture_own("];

    for file in collect_rs_files(src_dir) {
        let rel_path = file.strip_prefix(src_dir).unwrap_or(&file);
        if rel_path == Path::new("tmux/client.rs")
            || rel_path == Path::new("tmux\\client.rs")
            || rel_path == Path::new("commands.rs")
        {
            continue;
        }

        let content = fs::read_to_string(&file).unwrap_or_default();
        for (line_no, line) in content.lines().enumerate() {
            for pattern in &patterns {
                if line.contains(pattern) {
                    violations.push(format!(
                        "{}:{}: own pane helper '{}' called outside commands.rs",
                        rel_path.display(),
                        line_no + 1,
                        pattern
                    ));
                }
            }
        }
    }

    violations
}

pub fn check_no_shell(src_dir: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let shell_patterns = [
        "Command::new(\"sh\")",
        "Command::new(\"bash\")",
        "Command::new(\"zsh\")",
        "Command::new(\"cmd\")",
        "Command::new(\"cmd.exe\")",
        "Command::new(\"/bin/sh\")",
        "Command::new(\"/bin/bash\")",
    ];

    for file in collect_rs_files(src_dir) {
        let rel_path = file.strip_prefix(src_dir).unwrap_or(&file);
        if rel_path == Path::new("quota.rs") {
            continue;
        }

        let content = fs::read_to_string(&file).unwrap_or_default();
        for (line_no, line) in content.lines().enumerate() {
            for pattern in &shell_patterns {
                if line.contains(pattern) {
                    violations.push(format!(
                        "{}:{}: unauthorized shell invocation '{}'",
                        rel_path.display(),
                        line_no + 1,
                        pattern
                    ));
                }
            }
            if line.contains(".arg(\"-c\")") || line.contains(".arg(\"/c\")") {
                violations.push(format!(
                    "{}:{}: unauthorized shell flag '-c'",
                    rel_path.display(),
                    line_no + 1
                ));
            }
        }
    }

    violations
}

pub fn check_no_network(cargo_toml_path: &Path, src_dir: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let banned_crates = [
        "reqwest",
        "hyper",
        "tokio-tungstenite",
        "tungstenite",
        "ureq",
        "surf",
        "attohttpc",
        "isahc",
        "curl",
        "socket2",
        "tungstenite",
    ];

    if cargo_toml_path.exists() {
        let toml_content = fs::read_to_string(cargo_toml_path).unwrap_or_default();
        for (line_no, line) in toml_content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                continue;
            }
            for banned in &banned_crates {
                let prefix1 = format!("{banned} =");
                let prefix2 = format!("{banned}=");
                let prefix3 = format!("\"{banned}\"");
                if trimmed.starts_with(&prefix1)
                    || trimmed.starts_with(&prefix2)
                    || trimmed.contains(&prefix3)
                {
                    violations.push(format!(
                        "Cargo.toml:{}: banned network crate '{}'",
                        line_no + 1,
                        banned
                    ));
                }
            }
        }
    }

    let banned_std = [
        "std::net",
        "TcpStream",
        "TcpListener",
        "UdpSocket",
        "use std::net",
    ];

    for file in collect_rs_files(src_dir) {
        let rel_path = file.strip_prefix(src_dir).unwrap_or(&file);
        let content = fs::read_to_string(&file).unwrap_or_default();
        for (line_no, line) in content.lines().enumerate() {
            for banned in &banned_std {
                if line.contains(banned) {
                    violations.push(format!(
                        "{}:{}: banned network import or symbol '{}'",
                        rel_path.display(),
                        line_no + 1,
                        banned
                    ));
                }
            }
        }
    }

    violations
}

#[derive(Debug, PartialEq, Eq)]
struct FunctionRange {
    name: String,
    start_line: usize,
    end_line: usize,
}

fn extract_functions(source: &str) -> Vec<FunctionRange> {
    let mut functions = Vec::new();
    let chars: Vec<char> = source.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut line_no = 1;

    while i < len {
        if chars[i] == '\n' {
            line_no += 1;
            i += 1;
            continue;
        }

        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '/' {
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                if chars[i] == '\n' {
                    line_no += 1;
                }
                i += 1;
            }
            i += 2;
            continue;
        }

        if chars[i] == '"' {
            i += 1;
            while i < len && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < len {
                    i += 1;
                }
                if chars[i] == '\n' {
                    line_no += 1;
                }
                i += 1;
            }
            if i < len {
                i += 1;
            }
            continue;
        }

        if chars[i] == 'r' && i + 1 < len && (chars[i + 1] == '"' || chars[i + 1] == '#') {
            i += 1;
            let mut hashes = 0;
            while i < len && chars[i] == '#' {
                hashes += 1;
                i += 1;
            }
            if i < len && chars[i] == '"' {
                i += 1;
                loop {
                    if i >= len {
                        break;
                    }
                    if chars[i] == '"' {
                        let mut closing_hashes = 0;
                        while i + 1 + closing_hashes < len
                            && chars[i + 1 + closing_hashes] == '#'
                            && closing_hashes < hashes
                        {
                            closing_hashes += 1;
                        }
                        if closing_hashes == hashes {
                            i += 1 + hashes;
                            break;
                        }
                    }
                    if chars[i] == '\n' {
                        line_no += 1;
                    }
                    i += 1;
                }
            }
            continue;
        }

        if chars[i] == 'f' && i + 1 < len && chars[i + 1] == 'n' {
            let before_boundary = i == 0 || !chars[i - 1].is_alphanumeric() && chars[i - 1] != '_';
            let after_boundary =
                i + 2 < len && !chars[i + 2].is_alphanumeric() && chars[i + 2] != '_';
            if before_boundary && after_boundary {
                let fn_decl_line = line_no;
                i += 2;
                while i < len && chars[i].is_whitespace() {
                    if chars[i] == '\n' {
                        line_no += 1;
                    }
                    i += 1;
                }

                let mut fn_name = String::new();
                while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    fn_name.push(chars[i]);
                    i += 1;
                }

                while i < len && chars[i] != '{' && chars[i] != ';' {
                    if chars[i] == '\n' {
                        line_no += 1;
                    }
                    i += 1;
                }

                if i < len && chars[i] == '{' {
                    let body_start_line = line_no;
                    let mut depth = 1;
                    i += 1;

                    while i < len && depth > 0 {
                        if chars[i] == '\n' {
                            line_no += 1;
                            i += 1;
                            continue;
                        }
                        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '/' {
                            while i < len && chars[i] != '\n' {
                                i += 1;
                            }
                            continue;
                        }
                        if chars[i] == '/' && i + 1 < len && chars[i + 1] == '*' {
                            i += 2;
                            while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                                if chars[i] == '\n' {
                                    line_no += 1;
                                }
                                i += 1;
                            }
                            i += 2;
                            continue;
                        }
                        if chars[i] == '"' {
                            i += 1;
                            while i < len && chars[i] != '"' {
                                if chars[i] == '\\' && i + 1 < len {
                                    i += 1;
                                }
                                if chars[i] == '\n' {
                                    line_no += 1;
                                }
                                i += 1;
                            }
                            if i < len {
                                i += 1;
                            }
                            continue;
                        }
                        if chars[i] == '{' {
                            depth += 1;
                        } else if chars[i] == '}' {
                            depth -= 1;
                        }
                        i += 1;
                    }

                    if !fn_name.is_empty() {
                        functions.push(FunctionRange {
                            name: fn_name,
                            start_line: fn_decl_line.min(body_start_line),
                            end_line: line_no,
                        });
                    }
                    continue;
                }
            }
        }

        i += 1;
    }

    functions
}

pub fn check_respawn_pane(src_dir: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let allowed_functions: HashSet<&str> = [
        "restart",
        "_respawn",
        "respawn",
        "close_agent",
        "respawn_own",
    ]
    .into_iter()
    .collect();

    for file in collect_rs_files(src_dir) {
        let rel_path = file.strip_prefix(src_dir).unwrap_or(&file);
        let content = fs::read_to_string(&file).unwrap_or_default();

        let rel_str = rel_path.to_string_lossy();
        let is_layout = rel_str.ends_with("layout.rs");
        let is_client = rel_str.ends_with("client.rs");

        let functions = extract_functions(&content);

        for (line_no, line) in content.lines().enumerate() {
            let actual_line = line_no + 1;
            if line.contains("respawn-pane") {
                let enclosing = functions
                    .iter()
                    .find(|f| actual_line >= f.start_line && actual_line <= f.end_line);

                match enclosing {
                    Some(func) => {
                        if !is_layout || !allowed_functions.contains(func.name.as_str()) {
                            violations.push(format!(
                                "{}:{}: respawn-pane in unauthorized function '{}'",
                                rel_path.display(),
                                actual_line,
                                func.name
                            ));
                        }
                    }
                    None => {
                        if !is_client {
                            violations.push(format!(
                                "{}:{}: respawn-pane at module level outside client.rs",
                                rel_path.display(),
                                actual_line
                            ));
                        }
                    }
                }
            }
        }
    }

    violations
}

#[test]
fn test_forbidden_tmux_operations_only_appear_in_the_guarded_client() {
    let src = manifest_dir().join("src");
    let violations = check_forbidden_tmux_operations(&src);
    assert_eq!(violations, Vec::<String>::new());
}

#[test]
fn test_own_pane_helpers_are_only_called_for_tandem_panes() {
    let src = manifest_dir().join("src");
    let violations = check_own_pane_helpers(&src);
    assert_eq!(violations, Vec::<String>::new());
}

#[test]
fn test_no_shell_outside_quota_chain() {
    let src = manifest_dir().join("src");
    let violations = check_no_shell(&src);
    assert_eq!(violations, Vec::<String>::new());
}

#[test]
fn test_no_network_crates_or_std_net() {
    let cargo = manifest_dir().join("Cargo.toml");
    let src = manifest_dir().join("src");
    let violations = check_no_network(&cargo, &src);
    assert_eq!(violations, Vec::<String>::new());
}

#[test]
fn test_respawn_pane_only_runs_through_the_layout_restart_paths() {
    let src = manifest_dir().join("src");
    let violations = check_respawn_pane(&src);
    assert_eq!(violations, Vec::<String>::new());
}

#[test]
fn test_negative_tos_violations_are_detected() {
    let tmp = std::env::temp_dir().join(format!("tos_guard_neg_{}", uuid::Uuid::new_v4()));
    let tmp_src = tmp.join("src");
    fs::create_dir_all(&tmp_src).unwrap();

    fs::write(
        tmp_src.join("bad_tmux.rs"),
        "pub fn hack() { let _ = \"send-keys\"; }",
    )
    .unwrap();
    let v1 = check_forbidden_tmux_operations(&tmp_src);
    assert_eq!(v1.len(), 1);
    assert!(v1[0].contains("send-keys"));

    fs::write(
        tmp_src.join("bad_helper.rs"),
        "pub fn hack(c: &Client) { c.send_keys_own(\"%1\", \"bad\"); }",
    )
    .unwrap();
    let v2 = check_own_pane_helpers(&tmp_src);
    assert_eq!(v2.len(), 1);
    assert!(v2[0].contains(".send_keys_own("));

    fs::write(
        tmp_src.join("bad_shell.rs"),
        "pub fn run() { let _ = std::process::Command::new(\"bash\"); }",
    )
    .unwrap();
    let v3 = check_no_shell(&tmp_src);
    assert_eq!(v3.len(), 1);
    assert!(v3[0].contains("Command::new(\"bash\")"));

    let bad_cargo = tmp.join("Cargo.toml");
    fs::write(&bad_cargo, "[dependencies]\nreqwest = \"0.12\"\n").unwrap();
    fs::write(
        tmp_src.join("bad_net.rs"),
        "use std::net::TcpStream;\npub fn ping() {}",
    )
    .unwrap();
    let v4 = check_no_network(&bad_cargo, &tmp_src);
    assert!(v4.iter().any(|v| v.contains("reqwest")));
    assert!(v4.iter().any(|v| v.contains("std::net")));

    fs::write(
        tmp_src.join("bad_respawn.rs"),
        "pub fn arbitrary_kill() { let _ = \"respawn-pane\"; }",
    )
    .unwrap();
    let v5 = check_respawn_pane(&tmp_src);
    assert_eq!(v5.len(), 1);
    assert!(v5[0].contains("arbitrary_kill"));

    let _ = fs::remove_dir_all(&tmp);
}
