use std::collections::{HashMap, HashSet};

use crate::selfexec::{self_command, shlex_quote};

pub const MODIFIERS: [&str; 3] = ["Ctrl", "Alt", "Shift"];

pub const RESIZE_TABLE: &str = "tandem-resize";
pub const RESIZE_STEP_COLS: usize = 5;
pub const RESIZE_STEP_ROWS: usize = 2;

pub const STATUS_REFRESH: &str = "Alt+r";
pub const GO_TO_AGENT: &str = "Alt+g";
pub const WITHDRAW: &str = "Alt+w";
pub const WIZARD_ADD: &str = "Alt+a";
pub const WIZARD_DELETE: &str = "Alt+d";
pub const WIZARD_FILTER: &str = "Alt+f";
pub const WIZARD_HELP: &str = "Alt+h";

pub const CLICK_ACTIONS: [&str; 14] = [
    "help",
    "add-agent",
    "prev",
    "next",
    "status",
    "queries",
    "planner",
    "implementer",
    "close-agent",
    "quit",
    "restart",
    "resize",
    "sidebar",
    "settings",
];

const MACOS_OPTION_CHARS: [(&str, char); 15] = [
    ("h", '\u{02d9}'),
    ("s", '\u{00df}'),
    ("q", '\u{0153}'),
    ("1", '\u{00a1}'),
    ("2", '\u{2122}'),
    ("g", '\u{00a9}'),
    ("r", '\u{00ae}'),
    ("w", '\u{2211}'),
    ("f", '\u{0192}'),
    ("a", '\u{00e5}'),
    ("d", '\u{2202}'),
    ("x", '\u{2248}'),
    ("z", '\u{03a9}'),
    ("b", '\u{222b}'),
    (",", '\u{2264}'),
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KeyParseError {
    #[error("Shortcut cannot be empty")]
    Empty,
    #[error("Shortcut must have at least one modifier key (Ctrl, Alt, Shift): '{0}'")]
    NoModifier(String),
    #[error("Shortcut has no base key: '{0}'")]
    NoBaseKey(String),
    #[error("Unknown modifier '{0}' in shortcut '{1}'")]
    UnknownModifier(String, String),
    #[error("Duplicate modifier '{0}' in shortcut '{1}'")]
    DuplicateModifier(String, String),
    #[error("Invalid empty modifier token in shortcut: '{0}'")]
    EmptyModifierToken(String),
    #[error("Shortcut base key cannot be empty")]
    EmptyBaseKey,
}

fn canonical_base_key(base: &str) -> Result<String, KeyParseError> {
    let cleaned = base.trim();
    if cleaned.is_empty() {
        return Err(KeyParseError::EmptyBaseKey);
    }
    let low = cleaned.to_lowercase();
    match low.as_str() {
        "up" => Ok("Up".to_string()),
        "down" => Ok("Down".to_string()),
        "left" => Ok("Left".to_string()),
        "right" => Ok("Right".to_string()),
        "enter" => Ok("Enter".to_string()),
        "escape" | "esc" => Ok("Escape".to_string()),
        "tab" => Ok("Tab".to_string()),
        "space" => Ok("Space".to_string()),
        "backspace" => Ok("Backspace".to_string()),
        "comma" => Ok(",".to_string()),
        _ => {
            if low.starts_with('f') && low.len() > 1 && low[1..].chars().all(|c| c.is_ascii_digit())
            {
                Ok(format!("F{}", &low[1..]))
            } else if cleaned.chars().count() == 1
                && cleaned.chars().next().unwrap().is_alphabetic()
            {
                Ok(low)
            } else {
                Ok(cleaned.to_string())
            }
        }
    }
}

fn normalize_modifier(token: &str) -> Option<&'static str> {
    match token.trim().to_lowercase().as_str() {
        "ctrl" | "control" | "c" | "⌃" => Some("Ctrl"),
        "alt" | "opt" | "option" | "⌥" | "meta" | "m" => Some("Alt"),
        "shift" | "s" | "⇧" => Some("Shift"),
        _ => None,
    }
}

pub fn parse_shortcut(raw: &str) -> Result<(Vec<String>, String), KeyParseError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(KeyParseError::Empty);
    }

    let mut special_base: Option<&str> = None;
    let mut work = trimmed;

    if work.ends_with("++") {
        special_base = Some("+");
        work = &work[..work.len() - 2];
    } else if work.ends_with("+-") || work.ends_with("--") {
        special_base = Some("-");
        work = &work[..work.len() - 2];
    }

    let mut tokens: Vec<String> = Vec::new();
    loop {
        let chars: Vec<char> = work.chars().collect();
        if chars.is_empty() {
            break;
        }

        let mut token_len = 0;
        while token_len < chars.len() {
            let c = chars[token_len];
            if c.is_alphanumeric() || c == '_' || c == '⌥' || c == '⌃' || c == '⇧' {
                token_len += 1;
            } else {
                break;
            }
        }

        if token_len > 0
            && token_len < chars.len()
            && (chars[token_len] == '+' || chars[token_len] == '-')
        {
            let token_str: String = chars[..token_len].iter().collect();
            tokens.push(token_str);
            let byte_offset = work
                .char_indices()
                .nth(token_len + 1)
                .map(|(idx, _)| idx)
                .unwrap_or(work.len());
            work = &work[byte_offset..];
            if work.is_empty() {
                break;
            }
        } else {
            break;
        }
    }

    let base_candidate = if let Some(sp) = special_base {
        if !work.is_empty() {
            tokens.push(work.to_string());
        }
        sp
    } else {
        work
    };

    if tokens.is_empty() {
        return Err(KeyParseError::NoModifier(raw.to_string()));
    }

    if base_candidate.trim().is_empty() {
        return Err(KeyParseError::NoBaseKey(raw.to_string()));
    }

    let mut found_mods: HashSet<&'static str> = HashSet::new();
    for token in tokens {
        let clean = token.trim();
        if clean.is_empty() {
            return Err(KeyParseError::EmptyModifierToken(raw.to_string()));
        }
        let canonical_mod = normalize_modifier(clean)
            .ok_or_else(|| KeyParseError::UnknownModifier(token.clone(), raw.to_string()))?;
        if found_mods.contains(canonical_mod) {
            return Err(KeyParseError::DuplicateModifier(token, raw.to_string()));
        }
        found_mods.insert(canonical_mod);
    }

    let canonical_mods: Vec<String> = MODIFIERS
        .iter()
        .filter(|m| found_mods.contains(**m))
        .map(|s| s.to_string())
        .collect();
    let canonical_base = canonical_base_key(base_candidate)?;
    Ok((canonical_mods, canonical_base))
}

pub fn format_shortcut(raw: &str) -> Result<String, KeyParseError> {
    let (mods, base) = parse_shortcut(raw)?;
    let mut parts = mods;
    parts.push(base);
    Ok(parts.join("+"))
}

pub fn tmux_key(raw: &str) -> Result<String, KeyParseError> {
    let (mods, base) = parse_shortcut(raw)?;
    let mut out = String::new();
    for m in mods {
        match m.as_str() {
            "Ctrl" => out.push_str("C-"),
            "Alt" => out.push_str("M-"),
            "Shift" => out.push_str("S-"),
            _ => {}
        }
    }
    out.push_str(&base);
    Ok(out)
}

pub fn alt_name(platform: &str) -> &'static str {
    if platform == "darwin" { "Opt" } else { "Alt" }
}

pub fn display_shortcut(raw: &str, platform: &str) -> Result<String, KeyParseError> {
    let (mods, base) = parse_shortcut(raw)?;
    let mut parts: Vec<String> = mods
        .into_iter()
        .map(|m| {
            if m == "Alt" {
                alt_name(platform).to_string()
            } else {
                m
            }
        })
        .collect();
    parts.push(base);
    Ok(parts.join("+"))
}

pub fn option_char(raw: &str, platform: &str) -> Option<char> {
    if platform != "darwin" {
        return None;
    }
    let (mods, base) = parse_shortcut(raw).ok()?;
    if mods.len() == 1 && mods[0] == "Alt" {
        let low = base.to_lowercase();
        for (k, c) in &MACOS_OPTION_CHARS {
            if *k == low {
                return Some(*c);
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Binding {
    pub key: String,
    pub group: String,
    pub description: String,
    pub hint: String,
    pub command: String,
}

impl Binding {
    pub fn new(
        key: &str,
        group: &str,
        description: &str,
        hint: &str,
        command: &str,
    ) -> Result<Self, KeyParseError> {
        let canonical = format_shortcut(key)?;
        Ok(Self {
            key: canonical,
            group: group.to_string(),
            description: description.to_string(),
            hint: hint.to_string(),
            command: command.to_string(),
        })
    }

    pub fn modifiers(&self) -> Result<Vec<String>, KeyParseError> {
        let (mods, _) = parse_shortcut(&self.key)?;
        Ok(mods)
    }

    pub fn base_key(&self) -> Result<String, KeyParseError> {
        let (_, base) = parse_shortcut(&self.key)?;
        Ok(base)
    }

    pub fn tmux_key(&self) -> Result<String, KeyParseError> {
        tmux_key(&self.key)
    }

    pub fn display_key(&self, platform: &str) -> Result<String, KeyParseError> {
        display_shortcut(&self.key, platform)
    }

    pub fn option_char(&self, platform: &str) -> Option<char> {
        option_char(&self.key, platform)
    }
}

pub fn default_keymap() -> Vec<Binding> {
    vec![
        Binding::new(
            "Alt+h",
            "Help",
            "open this help",
            "Help",
            "display-popup -E -w 70% -h 70% \"{tandem} help\"",
        )
        .unwrap(),
        Binding::new(
            "Ctrl+Alt+n",
            "Agents",
            "add an agent",
            "New agent",
            "display-popup -E -w 90% -h 90% \"{tandem} add-agent\"",
        )
        .unwrap(),
        Binding::new(
            "Alt+Up",
            "Tabs",
            "previous agent (sidebar order), shown alone",
            "Prev",
            "run-shell -b \"{tandem} tab prev\"",
        )
        .unwrap(),
        Binding::new(
            "Alt+Down",
            "Tabs",
            "next agent (sidebar order), shown alone",
            "Next",
            "run-shell -b \"{tandem} tab next\"",
        )
        .unwrap(),
        Binding::new(
            "Alt+s",
            "Status",
            "focus the status window",
            "Status",
            "run-shell -b \"{tandem} focus status\"",
        )
        .unwrap(),
        Binding::new(
            "Alt+q",
            "Status",
            "open the Queries tab",
            "Queries",
            "run-shell -b \"{tandem} focus queries\"",
        )
        .unwrap(),
        Binding::new(
            "Alt+1",
            "Columns",
            "show last planner",
            "Planner",
            "run-shell -b \"{tandem} focus planner\"",
        )
        .unwrap(),
        Binding::new(
            "Alt+2",
            "Columns",
            "show last implementer",
            "Implementer",
            "run-shell -b \"{tandem} focus implementer\"",
        )
        .unwrap(),
        Binding::new(
            "Ctrl+Alt+w",
            "Agents",
            "close the focused agent (asks first)",
            "Close",
            "confirm-before -p 'Close this agent? (y/n)' \"run-shell -b \\\"{tandem2} close-agent\\\"\"",
        )
        .unwrap(),
        Binding::new(
            "Ctrl+Alt+x",
            "Quit",
            "exit menu: save and exit, detach, update tandem, discard",
            "Quit",
            "run-shell -b \"{tandem} quit\"",
        )
        .unwrap(),
        Binding::new(
            "Ctrl+Alt+r",
            "Agents",
            "restart an exited agent",
            "Restart",
            "run-shell -b \"{tandem} restart\"",
        )
        .unwrap(),
        Binding::new(
            "Ctrl+Alt+z",
            "Layout",
            "resize mode: arrows resize the focused pane, = resets, Enter exits",
            "Resize",
            "switch-client -T tandem-resize",
        )
        .unwrap(),
        Binding::new(
            "Alt+b",
            "Sidebar",
            "focus the sidebar (popup when hidden)",
            "Sidebar",
            "run-shell -b \"{tandem} focus sidebar\"",
        )
        .unwrap(),
        Binding::new(
            "Alt+,",
            "Settings",
            "open Settings (grid, sidebar, Reset UI)",
            "Settings",
            "run-shell -b \"{tandem} focus settings\"",
        )
        .unwrap(),
    ]
}

pub fn action_index(action: &str) -> Option<usize> {
    match action.trim().to_lowercase().as_str() {
        "help" => Some(0),
        "add-agent" | "new-agent" => Some(1),
        "tab-prev" | "prev" => Some(2),
        "tab-next" | "next" => Some(3),
        "status" | "focus-status" => Some(4),
        "queries" | "focus-queries" => Some(5),
        "planner" | "focus-planner" | "planners" => Some(6),
        "implementer" | "focus-implementer" | "implementers" => Some(7),
        "close" | "close-agent" => Some(8),
        "quit" => Some(9),
        "restart" => Some(10),
        "resize" => Some(11),
        "sidebar" | "focus-sidebar" => Some(12),
        "settings" | "focus-settings" => Some(13),
        _ => None,
    }
}

pub fn keymap(env: Option<&HashMap<String, String>>) -> Result<Vec<Binding>, KeyParseError> {
    let raw = match env {
        Some(map) => map.get("TANDEM_KEYS").cloned().unwrap_or_default(),
        None => std::env::var("TANDEM_KEYS").unwrap_or_default(),
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(default_keymap());
    }

    let mut bindings = default_keymap();
    let raw_parts: Vec<&str> = trimmed.split(',').collect();
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < raw_parts.len() {
        let part = raw_parts[i];
        if (part.ends_with('+') || part.ends_with('-')) && i + 1 < raw_parts.len() {
            parts.push(format!("{},{}", part, raw_parts[i + 1]));
            i += 2;
        } else {
            parts.push(part.to_string());
            i += 1;
        }
    }

    for item_str in &parts {
        let item = item_str.trim();
        if item.is_empty() {
            continue;
        }
        let delim = if item.contains('=') {
            Some('=')
        } else if item.contains(':') {
            Some(':')
        } else {
            None
        };

        if let Some(d) = delim {
            let mut pieces = item.splitn(2, d);
            let action_name = pieces.next().unwrap_or("");
            let shortcut = pieces.next().unwrap_or("");
            if let Some(idx) = action_index(action_name) {
                let canonical = format_shortcut(shortcut.trim())?;
                bindings[idx].key = canonical;
            }
        }
    }
    Ok(bindings)
}

pub fn key_for(action: &str, km: &[Binding]) -> String {
    if let Some(idx) = action_index(action)
        && idx < km.len()
    {
        return km[idx].key.clone();
    }
    String::new()
}

pub fn hint_bar(platform: &str, km: &[Binding]) -> String {
    let mut parts = Vec::new();
    for b in km {
        if !b.hint.is_empty() {
            let key = b.display_key(platform).unwrap_or_else(|_| b.key.clone());
            parts.push(format!("{key} {}", b.hint));
        }
    }
    parts.join("  ")
}

pub fn hint_bar_with_ranges(platform: &str, km: &[Binding]) -> String {
    let mut parts = Vec::new();
    for (b, action) in km.iter().zip(CLICK_ACTIONS.iter()) {
        if !b.hint.is_empty() {
            let key = b.display_key(platform).unwrap_or_else(|_| b.key.clone());
            parts.push(format!(
                "#[range=user|{}]{} {}#[norange]",
                action, key, b.hint
            ));
        }
    }
    parts.join("  ")
}

pub fn resize_hint() -> &'static str {
    "RESIZE  ←→ width  ↑↓ height  = reset  Enter/Esc done"
}

pub fn status_left(platform: &str, km: &[Binding]) -> String {
    format!(
        " #{{?#{{==:#{{client_key_table}},{RESIZE_TABLE}}},{resize_hint},{hint_bar}}} ",
        resize_hint = resize_hint(),
        hint_bar = hint_bar_with_ranges(platform, km)
    )
}

fn dq(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

pub fn run_shell(launcher: &str, subcommand: &str) -> String {
    format!("run-shell -b \"{} {subcommand}\"", dq(launcher))
}

pub fn popup(launcher: &str, subcommand: &str, width: &str, height: &str) -> String {
    format!(
        "display-popup -E -w {width} -h {height} \"{} {subcommand}\"",
        dq(launcher)
    )
}

pub fn expand_command(command: &str, launcher: &str) -> String {
    command
        .replace("{tandem2}", &dq(&dq(launcher)))
        .replace("{tandem}", &dq(launcher))
}

pub fn launcher_for(self_cmd: Option<&[&str]>, workspace: Option<&str>) -> String {
    let base = if let Some(cmd) = self_cmd {
        self_command(Some(cmd))
    } else {
        self_command(None)
    };

    if let Some(ws) = workspace {
        format!("env TANDEM_WORKSPACE={} {base}", shlex_quote(ws))
    } else {
        base
    }
}

fn mouse3_menu_cmd(launcher: &str, km: &[Binding]) -> String {
    let close_idx = action_index("close").unwrap();
    let add_idx = action_index("add-agent").unwrap();
    let help_idx = action_index("help").unwrap();

    let close_cmd = expand_command(&km[close_idx].command, launcher);
    let add_agent_cmd = expand_command(&km[add_idx].command, launcher);
    let help_cmd = expand_command(&km[help_idx].command, launcher);

    let q = |s: &str| format!("\"{}\"", dq(s));

    let items = [
        format!("{} f {}", q("Focus this pane"), q("select-pane -t =")),
        format!(
            "{} r {}",
            q("#{?pane_dead,,-}Restart agent"),
            q(&run_shell(launcher, "restart"))
        ),
        format!("{} w {}", q("Close agent…"), q(&close_cmd)),
        "\"\"".to_string(),
        format!("{} n {}", q("New agent"), q(&add_agent_cmd)),
        format!(
            "{} s {}",
            q("Status"),
            q(&run_shell(launcher, "focus status"))
        ),
        format!(
            "{} q {}",
            q("Queries"),
            q(&run_shell(launcher, "focus queries"))
        ),
        format!(
            "{} z {}",
            q("Resize mode"),
            q(&format!("switch-client -T {RESIZE_TABLE}"))
        ),
        format!("{} h {}", q("Help"), q(&help_cmd)),
    ];
    let menu = format!(
        "display-menu -O -T tandem -t = -x M -y M {}",
        items.join(" ")
    );
    format!("select-pane -t = ; {menu}")
}

pub fn mouse_commands(
    self_cmd: Option<&[&str]>,
    workspace: Option<&str>,
    _platform: &str,
    km: &[Binding],
) -> Vec<Vec<String>> {
    let launcher = launcher_for(self_cmd, workspace);

    let click1_status = vec![
        "bind-key".to_string(),
        "-n".to_string(),
        "MouseDown1Status".to_string(),
        format!(
            "run-shell -b \"{} click #{{q:mouse_status_range}}\"",
            dq(&launcher)
        ),
    ];

    let click3_pane = vec![
        "bind-key".to_string(),
        "-n".to_string(),
        "MouseDown3Pane".to_string(),
        mouse3_menu_cmd(&launcher, km),
    ];

    let click1_border = vec![
        "bind-key".to_string(),
        "-n".to_string(),
        "MouseDown1Border".to_string(),
        "select-pane -t =".to_string(),
    ];
    let dclick1_border = vec![
        "bind-key".to_string(),
        "-n".to_string(),
        "DoubleClick1Border".to_string(),
        format!(
            "select-pane -t = ; {}",
            run_shell(&launcher, "equalize #{q:pane_id}")
        ),
    ];

    vec![
        click1_status,
        click3_pane,
        click1_border,
        dclick1_border,
        vec![
            "unbind-key".to_string(),
            "-q".to_string(),
            "-n".to_string(),
            "MouseDown3Status".to_string(),
        ],
        vec![
            "unbind-key".to_string(),
            "-q".to_string(),
            "-n".to_string(),
            "MouseDown3StatusLeft".to_string(),
        ],
        vec![
            "unbind-key".to_string(),
            "-q".to_string(),
            "-n".to_string(),
            "M-MouseDown3Pane".to_string(),
        ],
    ]
}

pub fn resize_commands(self_cmd: Option<&[&str]>, workspace: Option<&str>) -> Vec<Vec<String>> {
    let stay = format!("switch-client -T {RESIZE_TABLE}");
    let launcher = launcher_for(self_cmd, workspace);
    let bindings = [
        (
            "Left",
            format!("resize-pane -L {RESIZE_STEP_COLS} ; {stay}"),
        ),
        (
            "Right",
            format!("resize-pane -R {RESIZE_STEP_COLS} ; {stay}"),
        ),
        ("Up", format!("resize-pane -U {RESIZE_STEP_ROWS} ; {stay}")),
        (
            "Down",
            format!("resize-pane -D {RESIZE_STEP_ROWS} ; {stay}"),
        ),
        (
            "=",
            format!("{} ; {stay}", run_shell(&launcher, "reset-layout")),
        ),
        (
            "e",
            format!("{} ; {stay}", run_shell(&launcher, "equalize")),
        ),
        ("Enter", "switch-client -T root".to_string()),
        ("Escape", "switch-client -T root".to_string()),
        ("q", "switch-client -T root".to_string()),
    ];
    bindings
        .iter()
        .map(|(key, command)| {
            vec![
                "bind-key".to_string(),
                "-T".to_string(),
                RESIZE_TABLE.to_string(),
                key.to_string(),
                command.clone(),
            ]
        })
        .collect()
}

pub fn bind_commands(
    self_cmd: Option<&[&str]>,
    workspace: Option<&str>,
    platform: &str,
    km: &[Binding],
) -> Vec<Vec<String>> {
    let launcher = launcher_for(self_cmd, workspace);
    let mut result = Vec::new();

    for b in km {
        let command = expand_command(&b.command, &launcher);
        let tk = b.tmux_key().unwrap_or_else(|_| b.key.clone());
        result.push(vec![
            "bind-key".to_string(),
            "-n".to_string(),
            tk,
            command.clone(),
        ]);
        if let Some(ch) = b.option_char(platform) {
            result.push(vec![
                "bind-key".to_string(),
                "-n".to_string(),
                ch.to_string(),
                command,
            ]);
        }
    }

    result.push(vec![
        "bind-key".to_string(),
        "-n".to_string(),
        "M-PageDown".to_string(),
        run_shell(&launcher, "grid-scroll 1"),
    ]);
    result.push(vec![
        "bind-key".to_string(),
        "-n".to_string(),
        "M-NPage".to_string(),
        run_shell(&launcher, "grid-scroll 1"),
    ]);
    result.push(vec![
        "bind-key".to_string(),
        "-n".to_string(),
        "M-PageUp".to_string(),
        run_shell(&launcher, "grid-scroll -1"),
    ]);
    result.push(vec![
        "bind-key".to_string(),
        "-n".to_string(),
        "M-PPage".to_string(),
        run_shell(&launcher, "grid-scroll -1"),
    ]);

    result
}

fn global_help_lines(platform: &str, km: &[Binding]) -> Vec<String> {
    let mut groups: Vec<(String, Vec<&Binding>)> = Vec::new();
    for b in km {
        if let Some((_, list)) = groups.iter_mut().find(|(name, _)| *name == b.group) {
            list.push(b);
        } else {
            groups.push((b.group.clone(), vec![b]));
        }
    }

    let mut lines = vec!["Global (tmux)".to_string()];
    for (group, items) in groups {
        for (position, b) in items.into_iter().enumerate() {
            let label = if position == 0 { group.as_str() } else { "" };
            let key = b.display_key(platform).unwrap_or_else(|_| b.key.clone());
            lines.push(format!("  {label:<9} {key:<12} {}", b.description));
        }
    }
    lines
}

pub fn wizard_help_lines(platform: &str) -> Vec<String> {
    let d = |raw: &str| display_shortcut(raw, platform).unwrap_or_else(|_| raw.to_string());
    vec![
        format!("{} focus the model filter", d(WIZARD_FILTER)),
        format!("{} add another agent", d(WIZARD_ADD)),
        format!("{} delete the highlighted agent", d(WIZARD_DELETE)),
        "Shift+L launch all agents".to_string(),
        "Esc back or cancel".to_string(),
        format!("? or {} this help", d(WIZARD_HELP)),
    ]
}

pub fn help_lines(platform: &str, km: &[Binding]) -> Vec<String> {
    let d = |raw: &str| display_shortcut(raw, platform).unwrap_or_else(|_| raw.to_string());
    let resize_k = d(&key_for("resize", km));
    let mut lines = vec![
        "tandem keys (agents in sidebar, main grid, status below)".to_string(),
        String::new(),
    ];
    lines.extend(global_help_lines(platform, km));
    lines.push(String::new());
    lines.extend([
        "Sidebar & Grid".to_string(),
        format!(
            "  Grid      {} / {} scroll the grid one row",
            d("Alt+PgUp"),
            d("Alt+PgDn")
        ),
        format!(
            "  Equalize  double-click a border, or {} then e",
            d("Ctrl+Alt+z")
        ),
        String::new(),
        "Status & Queries window".to_string(),
        "  Status    Tab          switch between tabs (1-7 direct)".to_string(),
        format!("            {:<12} refresh the status now", d(STATUS_REFRESH)),
        format!(
            "            {:<12} go to the highlighted or asking agent's pane",
            d(GO_TO_AGENT)
        ),
        "  Queries   Enter        answer the selected open question".to_string(),
        "            1-9          fill the answer with a numbered option".to_string(),
        "            Ctrl+S       send the answer".to_string(),
        format!(
            "            {:<12} mark the selected question withdrawn",
            d(WITHDRAW)
        ),
        "            Esc          clear the draft and return to the list".to_string(),
        String::new(),
        "Launch / add-agent wizard".to_string(),
        format!("  Model     {:<12} focus the model filter", d(WIZARD_FILTER)),
        format!("  Summary   {:<12} add another agent", d(WIZARD_ADD)),
        format!(
            "            {:<12} delete the highlighted agent",
            d(WIZARD_DELETE)
        ),
        "            Shift+L      launch all configured agents".to_string(),
        "            Esc          back or cancel".to_string(),
        String::new(),
        "Mouse".to_string(),
        format!(
            "            click agent = show · Shift/{}+click = add to grid · right-click menu",
            alt_name(platform)
        ),
        "            double-click a border to equalize".to_string(),
        "            click a status-bar hint = press its key".to_string(),
        "            click a pane to focus it; drag a border to resize; scroll wheel scrolls"
            .to_string(),
        format!(
            "  Resize    {resize_k} then arrows (←→ {RESIZE_STEP_COLS} columns, ↑↓ {RESIZE_STEP_ROWS} rows) on the focused pane; = resets; Enter exits"
        ),
        format!(
            "  Custom    set TANDEM_KEYS=\"help={},quit={}\" (each needs at least one modifier)",
            d("Ctrl+Alt+h"),
            d("Ctrl+Alt+q")
        ),
        "  Detach    your agents keep running; run `tandem` again to re-attach".to_string(),
    ]);
    lines
}

pub fn crossterm_key(raw: &str) -> Result<crossterm::event::KeyEvent, KeyParseError> {
    let (mods, base) = parse_shortcut(raw)?;
    let mut modifiers = crossterm::event::KeyModifiers::empty();
    for m in &mods {
        match m.as_str() {
            "Ctrl" => modifiers |= crossterm::event::KeyModifiers::CONTROL,
            "Alt" => modifiers |= crossterm::event::KeyModifiers::ALT,
            "Shift" => modifiers |= crossterm::event::KeyModifiers::SHIFT,
            _ => {}
        }
    }

    let code = match base.as_str() {
        "Up" => crossterm::event::KeyCode::Up,
        "Down" => crossterm::event::KeyCode::Down,
        "Left" => crossterm::event::KeyCode::Left,
        "Right" => crossterm::event::KeyCode::Right,
        "Enter" => crossterm::event::KeyCode::Enter,
        "Escape" => crossterm::event::KeyCode::Esc,
        "Tab" => crossterm::event::KeyCode::Tab,
        "Backspace" => crossterm::event::KeyCode::Backspace,
        "Space" => crossterm::event::KeyCode::Char(' '),
        _ => {
            if base.starts_with('F') && base.len() > 1 {
                if let Ok(num) = base[1..].parse::<u8>() {
                    crossterm::event::KeyCode::F(num)
                } else {
                    crossterm::event::KeyCode::Null
                }
            } else if base.chars().count() == 1 {
                crossterm::event::KeyCode::Char(base.chars().next().unwrap())
            } else {
                crossterm::event::KeyCode::Null
            }
        }
    };

    Ok(crossterm::event::KeyEvent::new(code, modifiers))
}

pub fn matches_crossterm_event(
    raw: &str,
    event: &crossterm::event::KeyEvent,
    platform: &str,
) -> bool {
    if let Ok(expected) = crossterm_key(raw)
        && event.code == expected.code
        && event.modifiers == expected.modifiers
    {
        return true;
    }
    if let Some(opt_char) = option_char(raw, platform)
        && event.code == crossterm::event::KeyCode::Char(opt_char)
    {
        return true;
    }
    false
}
