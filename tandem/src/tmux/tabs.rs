use crate::tmux::keys::{default_keymap, display_shortcut, key_for};

pub const FORMAT: &str = "#{pane_id}\t#{window_name}\t#{@tandem_kind}\t#{@tandem_role}\t#{@tandem_index}\t#{@tandem_label}\t#{pane_dead}\t#{pane_dead_status}\t#{@tandem_session}\t#{@tandem_spec}\t#{@tandem_cli_version}";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PaneInfo {
    pub id: String,
    pub window: String,
    pub kind: String,
    pub role: String,
    pub index: usize,
    pub label: String,
    pub dead: bool,
    pub dead_status: Option<i32>,
    pub session: String,
    pub spec: String,
    pub cli_version: String,
}

impl PaneInfo {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &str,
        window: &str,
        kind: &str,
        role: &str,
        index: usize,
        label: &str,
        dead: bool,
        dead_status: Option<i32>,
    ) -> Self {
        Self {
            id: id.to_string(),
            window: window.to_string(),
            kind: kind.to_string(),
            role: role.to_string(),
            index,
            label: label.to_string(),
            dead,
            dead_status,
            session: String::new(),
            spec: String::new(),
            cli_version: String::new(),
        }
    }
}

pub fn parse_panes(text: &str) -> Vec<PaneInfo> {
    let mut panes = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 9 || parts.len() > 11 || parts[2].is_empty() || parts[3].is_empty() {
            continue;
        }
        let id = parts[0].to_string();
        let window = parts[1].to_string();
        let kind = parts[2].to_string();
        let role = parts[3].to_string();
        let index = parts[4].parse::<usize>().unwrap_or(0);
        let label = parts[5].to_string();
        let dead = parts[6] == "1";
        let dead_status = parts[7].parse::<i32>().ok();
        let session = parts[8].to_string();
        let spec = if parts.len() > 9 {
            parts[9].to_string()
        } else {
            String::new()
        };
        let cli_version = if parts.len() == 11 {
            parts[10].to_string()
        } else {
            String::new()
        };

        panes.push(PaneInfo {
            id,
            window,
            kind,
            role,
            index,
            label,
            dead,
            dead_status,
            session,
            spec,
            cli_version,
        });
    }
    panes
}

pub fn visible_pane<'a>(panes: &'a [PaneInfo], role: &str) -> Option<&'a PaneInfo> {
    panes
        .iter()
        .find(|p| p.role == role && (p.window == "main" || p.window == "tandem"))
}

pub fn agents_in<'a>(panes: &'a [PaneInfo], role: &str) -> Vec<&'a PaneInfo> {
    let mut agents: Vec<&'a PaneInfo> = panes
        .iter()
        .filter(|p| p.role == role && p.kind == "agent")
        .collect();
    agents.sort_by_key(|p| p.index);
    agents
}

pub fn neighbor<'a>(panes: &'a [PaneInfo], role: &str, step: isize) -> Option<&'a PaneInfo> {
    let agents = agents_in(panes, role);
    if agents.is_empty() {
        return None;
    }
    let current = visible_pane(panes, role);
    let position = current
        .and_then(|c| agents.iter().position(|p| p.id == c.id))
        .unwrap_or(0);
    let len = agents.len() as isize;
    let next_pos = ((position as isize + step) % len + len) % len;
    Some(agents[next_pos as usize])
}

pub fn safe_label(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric()
                || c == '.'
                || c == '_'
                || c == ':'
                || c == '·'
                || c == '-'
                || c == ' '
            {
                c
            } else {
                '?'
            }
        })
        .collect()
}

pub fn tab_text(pane: &PaneInfo) -> String {
    let base = format!("{} {}", pane.index, safe_label(&pane.label));
    if pane.dead {
        let status = match pane.dead_status {
            Some(s) => s.to_string(),
            None => "?".to_string(),
        };
        format!("⚠{base}(exited {status})")
    } else {
        base
    }
}

pub fn render_agent_title(pane: &PaneInfo) -> String {
    let active_marker = "#{?pane_active,▶ ,}";
    let cli = if !pane.spec.is_empty() {
        serde_json::from_str::<serde_json::Value>(&pane.spec)
            .ok()
            .and_then(|v| v.get("cli").and_then(|c| c.as_str()).map(|s| s.to_string()))
    } else {
        None
    };
    let cli_str = cli.unwrap_or_else(|| {
        if pane.label.contains('·') {
            pane.label
                .split('·')
                .next()
                .unwrap_or(&pane.label)
                .to_string()
        } else {
            pane.label.clone()
        }
    });
    let safe_cli = safe_label(&cli_str);
    let dead_suffix = if pane.dead {
        match pane.dead_status {
            Some(s) => format!(" (exited {s})"),
            None => " (exited ?)".to_string(),
        }
    } else {
        String::new()
    };
    format!(
        "─ {active_marker}{}-{} · {safe_cli}{dead_suffix} ─",
        pane.role, pane.index
    )
}

pub fn render_title(panes: &[PaneInfo], role: &str) -> String {
    let active_marker = "#{?pane_active,▶ ,}";
    if role == "sidebar" {
        return format!("─ {active_marker}tandem {} ↗ ─", env!("CARGO_PKG_VERSION"));
    }
    if role == "status" {
        return format!("─ {active_marker}STATUS ─");
    }
    let shown = visible_pane(panes, role)
        .filter(|p| p.kind == "agent")
        .or_else(|| {
            panes
                .iter()
                .find(|p| p.window == "main" && p.kind == "agent")
        });
    match shown {
        Some(pane) => render_agent_title(pane),
        None => {
            let add_key = display_shortcut(&key_for("add-agent", &default_keymap()), "linux")
                .unwrap_or_else(|_| "Ctrl+Alt+n".to_string());
            if role == "agent" || role == "main" || role.is_empty() {
                format!("─ {active_marker}No agent running. Press {add_key} to add one ─")
            } else {
                format!("─ {active_marker}No {role}s yet. Press {add_key} to add one ─")
            }
        }
    }
}
