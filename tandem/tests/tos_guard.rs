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

pub fn check_worker_pane_helpers(src_dir: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let patterns = [".send_keys_worker(", ".paste_worker(", ".capture_worker("];

    for file in collect_rs_files(src_dir) {
        let rel_path = file.strip_prefix(src_dir).unwrap_or(&file);
        let rel_str = rel_path.to_string_lossy().replace('\\', "/");
        if rel_str == "tmux/client.rs"
            || rel_str == "factory/team.rs"
            || rel_str.starts_with("factory/worker/")
        {
            continue;
        }

        let content = fs::read_to_string(&file).unwrap_or_default();
        for (line_no, line) in content.lines().enumerate() {
            for pattern in &patterns {
                if line.contains(pattern) {
                    violations.push(format!(
                        "{}:{}: worker pane helper '{}' called outside factory/worker/ and factory/team.rs",
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

pub fn check_factory_spawn_sites(src_dir: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let factory_dir = src_dir.join("factory");
    let broker_dir = factory_dir.join("broker");
    let spawn_markers = ["std::process", "Command::new"];
    let allowed_broker_spawners: &[(&str, Option<&[&str]>)] = &[
        ("mod.rs", Some(&[])),
        ("git.rs", Some(&["git"])),
        ("gh.rs", Some(&["gh"])),
        (
            "browser.rs",
            Some(&["chromium", "chromium-browser", "google-chrome"]),
        ),
        ("checks.rs", None),
    ];
    let gate_env_keywords = ["GATE", "BYPASS", "SKIP_TERMS", "OVERRIDE", "ALLOW_PROVIDER"];

    for file in collect_rs_files(&factory_dir) {
        let rel_path = file.strip_prefix(src_dir).unwrap_or(&file);
        let rel_str = rel_path.to_string_lossy().replace('\\', "/");
        let content = fs::read_to_string(&file).unwrap_or_default();
        let in_broker = file.starts_with(&broker_dir);
        let file_name = file.file_name().and_then(|s| s.to_str()).unwrap_or("");

        // Rule 1 & 2: spawning only under factory/broker/ and only in listed files/programs
        let mut spawns_in_file = false;
        for (line_no, line) in content.lines().enumerate() {
            for marker in &spawn_markers {
                if line.contains(marker) {
                    spawns_in_file = true;
                    if !in_broker {
                        violations.push(format!(
                            "{}:{}: process spawning outside factory/broker/ ('{}')",
                            rel_path.display(),
                            line_no + 1,
                            marker
                        ));
                    }
                }
            }
        }

        if in_broker && spawns_in_file {
            match allowed_broker_spawners
                .iter()
                .find(|(name, _)| *name == file_name)
            {
                None => {
                    violations.push(format!(
                        "{}: unlisted broker file spawns processes",
                        rel_path.display()
                    ));
                }
                Some((_, Some(allowed_progs))) => {
                    for (line_no, line) in content.lines().enumerate() {
                        if let Some(idx) = line.find("Command::new(\"") {
                            let rest = &line[idx + "Command::new(\"".len()..];
                            if let Some(end) = rest.find('"') {
                                let prog = &rest[..end];
                                if !allowed_progs.contains(&prog) {
                                    violations.push(format!(
                                        "{}:{}: broker file '{}' may not spawn '{}'",
                                        rel_path.display(),
                                        line_no + 1,
                                        file_name,
                                        prog
                                    ));
                                }
                            }
                        }
                    }
                }
                Some((_, None)) => {}
            }
        }

        // Rule 4: GatePass constructed only in factory/broker/gate.rs
        if rel_str != "factory/broker/gate.rs" {
            for (line_no, line) in content.lines().enumerate() {
                if line.contains("GatePass {") || line.contains("GatePass{") {
                    violations.push(format!(
                        "{}:{}: GatePass constructed outside gate.rs",
                        rel_path.display(),
                        line_no + 1
                    ));
                }
            }
        }

        // Rule 4b: ConfirmedChecks constructed only in factory/checks.rs
        if rel_str != "factory/checks.rs" {
            for (line_no, line) in content.lines().enumerate() {
                if line.contains("ConfirmedChecks {") || line.contains("ConfirmedChecks{") {
                    violations.push(format!(
                        "{}:{}: ConfirmedChecks constructed outside checks.rs",
                        rel_path.display(),
                        line_no + 1
                    ));
                }
            }
        }

        // Rule 5: no env-based gate decision under src/factory/
        let is_gate_or_terms =
            rel_str == "factory/broker/gate.rs" || rel_str == "factory/broker/terms.rs";
        for (line_no, line) in content.lines().enumerate() {
            let reads_env =
                line.contains("env::var") || line.contains("option_env!") || line.contains("env!(");
            if !reads_env {
                continue;
            }
            if is_gate_or_terms {
                violations.push(format!(
                    "{}:{}: gate/terms module must not read environment variables",
                    rel_path.display(),
                    line_no + 1
                ));
                continue;
            }
            let upper = line.to_ascii_uppercase();
            if gate_env_keywords.iter().any(|kw| upper.contains(kw)) {
                violations.push(format!(
                    "{}:{}: environment variable used to decide a gate",
                    rel_path.display(),
                    line_no + 1
                ));
            }
        }
    }

    // Rule 3b: run_checks in factory/broker/checks.rs must take a &ConfirmedChecks parameter
    let checks_rs = broker_dir.join("checks.rs");
    if checks_rs.exists() {
        let content = fs::read_to_string(&checks_rs).unwrap_or_default();
        let has_valid_sig = content.find("fn run_checks(").is_some_and(|start| {
            let sig_rest = &content[start..];
            let sig_end = sig_rest
                .find('{')
                .or_else(|| sig_rest.find("->"))
                .unwrap_or(sig_rest.len());
            let params = &sig_rest[..sig_end];
            params.contains("&ConfirmedChecks") && !params.contains("Option<&ConfirmedChecks>")
        });
        if !has_valid_sig {
            violations.push(
                "factory/broker/checks.rs: run_checks must take a required &ConfirmedChecks parameter"
                    .to_string(),
            );
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
fn test_worker_pane_helpers_only_called_from_factory_worker_or_team() {
    let src = manifest_dir().join("src");
    let violations = check_worker_pane_helpers(&src);
    assert_eq!(violations, Vec::<String>::new());
}

#[test]
fn test_login_module_never_reads_credentials_or_opens_network() {
    let login_rs = manifest_dir().join("src").join("factory").join("login.rs");
    let content = fs::read_to_string(&login_rs).expect("factory/login.rs must exist");
    let forbidden = [
        ".credentials.json",
        "auth.json",
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "GEMINI_API_KEY",
        "std::net",
        "reqwest",
        "ureq",
    ];
    for needle in forbidden {
        assert!(
            !content.contains(needle),
            "factory/login.rs must not reference {needle}"
        );
    }
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
fn test_factory_spawn_sites() {
    let src = manifest_dir().join("src");
    let violations = check_factory_spawn_sites(&src);
    assert_eq!(violations, Vec::<String>::new());
}

#[test]
fn test_outbound_writers_require_redacted() {
    let src = manifest_dir().join("src");
    let violations = check_outbound_writers_require_redacted(&src);
    assert_eq!(violations, Vec::<String>::new());
}

fn fn_sig_slice<'a>(content: &'a str, fn_name: &str) -> Option<&'a str> {
    let pattern = format!("fn {fn_name}(");
    let start = content.find(&pattern)?;
    let rest = &content[start..];
    let end = rest.find('{').unwrap_or(rest.len());
    Some(&rest[..end])
}

fn check_outbound_writers_require_redacted(src_dir: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let factory_dir = src_dir.join("factory");

    for file in collect_rs_files(&factory_dir) {
        let rel_path = file.strip_prefix(src_dir).unwrap_or(&file);
        let rel_str = rel_path.to_string_lossy().replace('\\', "/");
        if rel_str != "factory/redact.rs" {
            let content = fs::read_to_string(&file).unwrap_or_default();
            for (line_no, line) in content.lines().enumerate() {
                let trimmed = line.trim_end();
                let check_slice = trimmed.strip_suffix("-> Redacted {").unwrap_or(line);
                if check_slice.contains("Redacted {") || check_slice.contains("Redacted{") {
                    violations.push(format!(
                        "{}:{}: Redacted constructed outside redact.rs",
                        rel_path.display(),
                        line_no + 1
                    ));
                }
            }
        }
    }

    let events_rs = factory_dir.join("events.rs");
    if events_rs.exists() {
        let content = fs::read_to_string(&events_rs).unwrap_or_default();
        for fname in ["append", "append_locked"] {
            let ok = fn_sig_slice(&content, fname).is_some_and(|sig| sig.contains("RedactedEvent"));
            if !ok {
                violations.push(format!(
                    "factory/events.rs: {fname} must take RedactedEvent"
                ));
            }
        }
    }

    let evidence_rs = factory_dir.join("evidence.rs");
    if evidence_rs.exists() {
        let content = fs::read_to_string(&evidence_rs).unwrap_or_default();
        let has_redacted_summary = content
            .find("struct AddEvidenceInput")
            .is_some_and(|start| {
                let rest = &content[start..];
                let end = rest.find('}').unwrap_or(rest.len());
                rest[..end].contains("summary: Redacted")
            });
        if !has_redacted_summary {
            violations
                .push("factory/evidence.rs: AddEvidenceInput.summary must be Redacted".to_string());
        }
    }

    let handoff_rs = factory_dir.join("handoff.rs");
    if handoff_rs.exists() {
        let content = fs::read_to_string(&handoff_rs).unwrap_or_default();
        let build_ok = fn_sig_slice(&content, "build_handoff_markdown")
            .is_some_and(|sig| sig.contains("-> Redacted"));
        if !build_ok {
            violations.push(
                "factory/handoff.rs: build_handoff_markdown must return Redacted".to_string(),
            );
        }
        let write_ok =
            fn_sig_slice(&content, "write_handoff").is_some_and(|sig| sig.contains("&Redacted"));
        if !write_ok {
            violations.push("factory/handoff.rs: write_handoff must take &Redacted".to_string());
        }
    }

    let pr_rs = factory_dir.join("pr.rs");
    if pr_rs.exists() {
        let content = fs::read_to_string(&pr_rs).unwrap_or_default();
        for fname in ["build_pr_body", "conventional_pr_title"] {
            let ok = fn_sig_slice(&content, fname).is_some_and(|sig| sig.contains("-> Redacted"));
            if !ok {
                violations.push(format!("factory/pr.rs: {fname} must return Redacted"));
            }
        }
        let write_ok =
            fn_sig_slice(&content, "write_pr_body").is_some_and(|sig| sig.contains("&Redacted"));
        if !write_ok {
            violations.push("factory/pr.rs: write_pr_body must take &Redacted".to_string());
        }
    }

    let artifacts_rs = factory_dir.join("artifacts.rs");
    if artifacts_rs.exists() {
        let content = fs::read_to_string(&artifacts_rs).unwrap_or_default();
        for fname in ["put_redacted", "format_for_display"] {
            let ok = fn_sig_slice(&content, fname).is_some_and(|sig| sig.contains("&Redacted"));
            if !ok {
                violations.push(format!("factory/artifacts.rs: {fname} must take &Redacted"));
            }
        }
    }

    violations
}

#[test]
fn test_gh_broker_forbids_merge_and_auto_merge() {
    let factory_dir = manifest_dir().join("src").join("factory");
    for file in collect_rs_files(&factory_dir) {
        let content = fs::read_to_string(&file).unwrap_or_default();
        for (line_no, line) in content.lines().enumerate() {
            assert!(
                !line.contains("\"pr merge\"")
                    && !line.contains("\"merge\"")
                    && !line.contains("--auto"),
                "{}:{}: forbidden gh merge or auto-merge reference",
                file.display(),
                line_no + 1
            );
        }
    }
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

    let tmp_factory = tmp_src.join("factory");
    let tmp_broker = tmp_factory.join("broker");
    fs::create_dir_all(&tmp_broker).unwrap();

    // Rule 1: spawning outside factory/broker/
    fs::write(
        tmp_factory.join("bad_outside.rs"),
        "pub fn spawn() { let _ = std::process::Command::new(\"ls\"); }",
    )
    .unwrap();
    // Rule 2: unlisted broker file spawning
    fs::write(
        tmp_broker.join("rogue.rs"),
        "pub fn spawn() { let _ = std::process::Command::new(\"curl\"); }",
    )
    .unwrap();
    // Rule 4: GatePass constructed outside gate.rs
    fs::write(
        tmp_broker.join("forged.rs"),
        "pub fn forge() -> GatePass { GatePass { _sealed: () } }",
    )
    .unwrap();
    // Rule 5: env-based gate override
    fs::write(
        tmp_factory.join("bad_env_gate.rs"),
        "pub fn bypass() -> bool { std::env::var(\"TANDEM_BYPASS_GATE\").is_ok() }",
    )
    .unwrap();

    // Rule 3b: run_checks without &ConfirmedChecks
    fs::write(
        tmp_broker.join("checks.rs"),
        "pub fn run_checks(cmds: &[String]) {}",
    )
    .unwrap();
    // Rule 4b: ConfirmedChecks constructed outside checks.rs
    fs::write(
        tmp_broker.join("forged_checks.rs"),
        "pub fn forge() -> ConfirmedChecks { ConfirmedChecks { target: String::new() } }",
    )
    .unwrap();
    fs::write(
        tmp_broker.join("browser.rs"),
        "pub fn cap() { let _ = std::process::Command::new(\"curl\"); }",
    )
    .unwrap();

    let v6 = check_factory_spawn_sites(&tmp_src);
    assert!(
        v6.iter()
            .any(|v| v.contains("bad_outside.rs") && v.contains("outside factory/broker/")),
        "expected Rule 1 violation, got {v6:?}"
    );
    assert!(
        v6.iter()
            .any(|v| v.contains("rogue.rs") && v.contains("unlisted broker file")),
        "expected Rule 2 violation, got {v6:?}"
    );
    assert!(
        v6.iter()
            .any(|v| v.contains("forged.rs") && v.contains("outside gate.rs")),
        "expected Rule 4 violation, got {v6:?}"
    );
    assert!(
        v6.iter()
            .any(|v| v.contains("bad_env_gate.rs") && v.contains("decide a gate")),
        "expected Rule 5 violation, got {v6:?}"
    );
    assert!(
        v6.iter()
            .any(|v| v.contains("checks.rs") && v.contains("ConfirmedChecks parameter")),
        "expected Rule 3b violation, got {v6:?}"
    );
    assert!(
        v6.iter()
            .any(|v| v.contains("forged_checks.rs") && v.contains("outside checks.rs")),
        "expected Rule 4b violation, got {v6:?}"
    );
    assert!(
        v6.iter()
            .any(|v| v.contains("browser.rs") && v.contains("may not spawn 'curl'")),
        "expected browser.rs allowlist violation, got {v6:?}"
    );

    fs::write(
        tmp_factory.join("forged_redacted.rs"),
        "pub fn forge() -> Redacted { Redacted { text: String::new(), replacements: 0 } }",
    )
    .unwrap();
    fs::write(
        tmp_factory.join("events.rs"),
        "pub fn append(p: &Path, l: &Path, e: &[Event]) {}\npub fn append_locked(p: &Path, e: &[Event]) {}",
    )
    .unwrap();
    fs::write(
        tmp_factory.join("evidence.rs"),
        "pub struct AddEvidenceInput { pub summary: String }",
    )
    .unwrap();
    fs::write(
        tmp_factory.join("handoff.rs"),
        "pub fn build_handoff_markdown() -> String { String::new() }\npub fn write_handoff(s: &str) {}",
    )
    .unwrap();
    fs::write(
        tmp_factory.join("pr.rs"),
        "pub fn build_pr_body() -> String { String::new() }\npub fn conventional_pr_title() -> String { String::new() }\npub fn write_pr_body(s: &str) {}",
    )
    .unwrap();
    fs::write(
        tmp_factory.join("artifacts.rs"),
        "pub fn put_redacted(s: &str) {}\npub fn format_for_display(s: &str) -> &str { s }",
    )
    .unwrap();

    let v7 = check_outbound_writers_require_redacted(&tmp_src);
    assert!(
        v7.iter()
            .any(|v| v.contains("forged_redacted.rs") && v.contains("outside redact.rs")),
        "expected forged Redacted violation, got {v7:?}"
    );
    assert!(
        v7.iter()
            .any(|v| v.contains("events.rs") && v.contains("RedactedEvent")),
        "expected events.rs violation, got {v7:?}"
    );
    assert!(
        v7.iter()
            .any(|v| v.contains("evidence.rs") && v.contains("AddEvidenceInput.summary")),
        "expected evidence.rs violation, got {v7:?}"
    );
    assert!(
        v7.iter()
            .any(|v| v.contains("handoff.rs") && v.contains("build_handoff_markdown")),
        "expected handoff.rs build violation, got {v7:?}"
    );
    assert!(
        v7.iter()
            .any(|v| v.contains("pr.rs") && v.contains("write_pr_body")),
        "expected pr.rs write violation, got {v7:?}"
    );
    assert!(
        v7.iter()
            .any(|v| v.contains("artifacts.rs") && v.contains("put_redacted")),
        "expected artifacts.rs violation, got {v7:?}"
    );

    let tmp_worker_dir = tmp_factory.join("worker");
    fs::create_dir_all(&tmp_worker_dir).unwrap();
    fs::write(
        tmp_worker_dir.join("ok.rs"),
        "pub fn drive(c: &TmuxClient) { let _ = c.send_keys_worker(\"r\", \"%1\", &[\"Enter\"]); let _ = c.paste_worker(\"r\", \"%1\", \"hi\"); let _ = c.capture_worker(\"r\", \"%1\", 20); }",
    )
    .unwrap();
    fs::write(
        tmp_factory.join("team.rs"),
        "pub fn msg(c: &TmuxClient) { let _ = c.send_keys_worker(\"r\", \"%1\", &[\"Enter\"]); }",
    )
    .unwrap();
    fs::write(
        tmp_src.join("commands.rs"),
        "pub fn bad(c: &TmuxClient) { let _ = c.paste_worker(\"r\", \"%1\", \"hi\"); }",
    )
    .unwrap();
    fs::write(
        tmp_factory.join("bad_caller.rs"),
        "pub fn bad(c: &TmuxClient) { let _ = c.capture_worker(\"r\", \"%1\", 10); let _ = c.send_keys_worker(\"r\", \"%1\", &[\"Enter\"]); }",
    )
    .unwrap();

    let v8 = check_worker_pane_helpers(&tmp_src);
    assert_eq!(
        v8.len(),
        3,
        "expected 3 worker helper violations, got {v8:?}"
    );
    assert!(
        v8.iter()
            .any(|v| v.contains("commands.rs") && v.contains(".paste_worker("))
    );
    assert!(
        v8.iter()
            .any(|v| v.contains("bad_caller.rs") && v.contains(".capture_worker("))
    );
    assert!(
        v8.iter()
            .any(|v| v.contains("bad_caller.rs") && v.contains(".send_keys_worker("))
    );

    let _ = fs::remove_dir_all(&tmp);
}

fn collect_files_with_ext(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if dir.is_dir()
        && let Ok(entries) = fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_files_with_ext(&path, exts));
            } else if path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|e| exts.contains(&e))
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

pub fn check_no_noninteractive_ai_argv(src_dir: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let flag_tokens = [
        "\"--print\"",
        "\"--output-format\"",
        "\"stream-json\"",
        "\"--bare\"",
    ];
    let ai_clis = ["\"claude\"", "\"agy\"", "\"opencode\""];
    for file in collect_rs_files(src_dir) {
        let rel = file.strip_prefix(src_dir).unwrap_or(&file).to_path_buf();
        let content = fs::read_to_string(&file).unwrap_or_default();
        for (line_no, line) in content.lines().enumerate() {
            for token in flag_tokens {
                if line.contains(token) {
                    violations.push(format!(
                        "{}:{}: non-interactive AI CLI flag {token}",
                        rel.display(),
                        line_no + 1
                    ));
                }
            }
        }
        for cli in ai_clis {
            let mut from = 0;
            while let Some(pos) = content[from..].find(cli) {
                let at = from + pos;
                let window_end = (at + cli.len() + 160).min(content.len());
                let mut end = window_end;
                while !content.is_char_boundary(end) {
                    end -= 1;
                }
                let window = &content[at + cli.len()..end];
                let statement = window.split(';').next().unwrap_or(window);
                let flagged = if cli == "\"opencode\"" {
                    statement.contains("\"run\"")
                } else {
                    statement.contains("\"-p\"")
                };
                if flagged {
                    let line_no = content[..at].matches('\n').count() + 1;
                    violations.push(format!(
                        "{}:{}: {cli} launched with a non-interactive argument",
                        rel.display(),
                        line_no
                    ));
                }
                from = at + cli.len();
            }
        }
    }
    violations
}

pub fn check_no_headless_symbols(roots: &[(&Path, &[&str])]) -> Vec<String> {
    let mut violations = Vec::new();
    let forbidden = [
        "HeadlessAdapter",
        "HeadlessRequest",
        "build_headless_argv",
        "spawn_provider",
        "provider_gates",
        "ProviderGate",
        "allows_headless",
        "consent_provider",
        "factoryProviderStatus",
    ];
    for (root, allowed_files) in roots {
        for file in collect_files_with_ext(root, &["rs", "ts", "tsx"]) {
            let rel = file.strip_prefix(root).unwrap_or(&file).to_path_buf();
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if allowed_files.contains(&rel_str.as_str()) {
                continue;
            }
            let content = fs::read_to_string(&file).unwrap_or_default();
            for (line_no, line) in content.lines().enumerate() {
                for symbol in forbidden {
                    if line.contains(symbol) {
                        violations.push(format!(
                            "{}:{}: removed headless symbol {symbol}",
                            rel.display(),
                            line_no + 1
                        ));
                    }
                }
            }
        }
    }
    violations
}

#[test]
fn test_no_noninteractive_ai_cli_argv_in_the_shipped_source() {
    let src = manifest_dir().join("src");
    assert_eq!(check_no_noninteractive_ai_argv(&src), Vec::<String>::new());
}

#[test]
fn test_no_headless_symbols_remain_in_tandem_or_tandem_web() {
    let tandem_src = manifest_dir().join("src");
    let web_src = manifest_dir().join("..").join("tandem-web").join("src");
    let roots: Vec<(&Path, &[&str])> = vec![
        (tandem_src.as_path(), &["factory/settings.rs"]),
        (web_src.as_path(), &[]),
    ];
    assert_eq!(check_no_headless_symbols(&roots), Vec::<String>::new());
}

#[test]
fn test_interactive_only_guards_detect_violations() {
    let tmp = std::env::temp_dir().join(format!("tandem-tos-iguard-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    fs::write(
        tmp.join("bad_claude.rs"),
        "fn x() { let _ = vec![\"claude\".to_string(), \"-p\".to_string()]; }",
    )
    .unwrap();
    fs::write(
        tmp.join("bad_agy.rs"),
        "fn x() { let _ = [\"agy\", \"--print\"]; }",
    )
    .unwrap();
    fs::write(
        tmp.join("bad_opencode.rs"),
        "fn x() { let _ = [\"opencode\", \"run\", \"hi\"]; }",
    )
    .unwrap();
    fs::write(
        tmp.join("ok_interactive.rs"),
        "fn x() { let _ = [\"claude\", \"--permission-mode\", \"plan\"]; let _ = [\"opencode\", \"--model\", \"m\"]; }",
    )
    .unwrap();
    let v = check_no_noninteractive_ai_argv(&tmp);
    assert!(
        v.iter()
            .any(|m| m.contains("bad_claude.rs") && m.contains("\"claude\"")),
        "{v:?}"
    );
    assert!(v.iter().any(|m| m.contains("bad_agy.rs")), "{v:?}");
    assert!(
        v.iter()
            .any(|m| m.contains("bad_opencode.rs") && m.contains("\"opencode\"")),
        "{v:?}"
    );
    assert!(!v.iter().any(|m| m.contains("ok_interactive.rs")), "{v:?}");

    let sym = tmp.join("sym");
    fs::create_dir_all(&sym).unwrap();
    fs::write(sym.join("a.rs"), "pub trait HeadlessAdapter {}").unwrap();
    fs::write(sym.join("b.ts"), "const x = factoryProviderStatus;").unwrap();
    fs::write(sym.join("legacy.rs"), "extra.remove(\"provider_gates\");").unwrap();
    let roots: Vec<(&Path, &[&str])> = vec![(sym.as_path(), &["legacy.rs"])];
    let v = check_no_headless_symbols(&roots);
    assert!(
        v.iter()
            .any(|m| m.contains("a.rs") && m.contains("HeadlessAdapter")),
        "{v:?}"
    );
    assert!(
        v.iter()
            .any(|m| m.contains("b.ts") && m.contains("factoryProviderStatus")),
        "{v:?}"
    );
    assert!(!v.iter().any(|m| m.contains("legacy.rs")), "{v:?}");

    let _ = fs::remove_dir_all(&tmp);
}
