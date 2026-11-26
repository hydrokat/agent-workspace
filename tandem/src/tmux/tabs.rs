use crate::tmux::keys::{default_keymap, display_shortcut, key_for};

pub const FORMAT: &str = "#{pane_id}\t#{window_name}\t#{@tandem_kind}\t#{@tandem_role}\t#{@tandem_index}\t#{@tandem_label}\t#{pane_dead}\t#{pane_dead_status}\t#{@tandem_session}\t#{@tandem_spec}\t#{@tandem_cli_version}\t#{@tandem_name}\t#{@tandem_idea}\t#{@tandem_run}\t#{@tandem_job}\t#{@tandem_stage}\t#{@tandem_cli}\t#{@tandem_model}";

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub idea: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub job: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stage: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cli: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
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
            ..Self::default()
        }
    }
}

pub fn parse_panes(text: &str) -> Vec<PaneInfo> {
    let mut panes = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 9 || parts.len() > 18 || parts[2].is_empty() || parts[3].is_empty() {
            continue;
        }
        let field =
            |idx: usize| -> String { parts.get(idx).map(|s| (*s).to_string()).unwrap_or_default() };

        panes.push(PaneInfo {
            id: field(0),
            window: field(1),
            kind: field(2),
            role: field(3),
            index: parts[4].parse::<usize>().unwrap_or(0),
            label: field(5),
            dead: parts[6] == "1",
            dead_status: parts[7].parse::<i32>().ok(),
            session: field(8),
            spec: field(9),
            cli_version: field(10),
            name: field(11),
            idea: field(12),
            run: field(13),
            job: field(14),
            stage: field(15),
            cli: field(16),
            model: field(17),
        });
    }
    panes
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum AgentCategory {
    Orchestrators,
    Planners,
    Testers,
    Implementers,
    Reviewers,
    Ops,
}

impl AgentCategory {
    pub const fn all() -> [Self; 6] {
        [
            Self::Orchestrators,
            Self::Planners,
            Self::Testers,
            Self::Implementers,
            Self::Reviewers,
            Self::Ops,
        ]
    }

    pub const fn order(self) -> usize {
        match self {
            Self::Orchestrators => 0,
            Self::Planners => 1,
            Self::Testers => 2,
            Self::Implementers => 3,
            Self::Reviewers => 4,
            Self::Ops => 5,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Orchestrators => "Orchestrators",
            Self::Planners => "Planners",
            Self::Testers => "Testers",
            Self::Implementers => "Implementers",
            Self::Reviewers => "Reviewers",
            Self::Ops => "Ops",
        }
    }

    pub const fn sidebar_header(self) -> &'static str {
        match self {
            Self::Orchestrators => "ORCHESTRATORS",
            Self::Planners => "PLANNERS",
            Self::Testers => "TESTERS",
            Self::Implementers => "IMPLEMENTERS",
            Self::Reviewers => "REVIEWERS",
            Self::Ops => "OPS",
        }
    }
}

pub fn categorize_agent(role: &str, stage: &str) -> AgentCategory {
    let r = role.trim().to_ascii_lowercase();
    let s = stage.trim().to_ascii_lowercase();
    if r == "orchestrator" {
        return AgentCategory::Orchestrators;
    }
    // Check ops/specialty stages & roles before generic `engineer` matches so devops/release stay under Ops.
    if matches!(s.as_str(), "demo" | "pr" | "deploy")
        || r.contains("devops")
        || r.contains("release")
        || r.contains("demo")
    {
        return AgentCategory::Ops;
    }
    if s == "plan" || r.contains("planner") || r.contains("architect") || r.contains("analyst") {
        return AgentCategory::Planners;
    }
    if matches!(s.as_str(), "create-tests" | "test")
        || r.contains("test")
        || r.contains("qa")
        || r.contains("verifier")
    {
        return AgentCategory::Testers;
    }
    if s == "review" || r.contains("review") || r.contains("security") || r.contains("audit") {
        return AgentCategory::Reviewers;
    }
    if s == "develop"
        || r.contains("implement")
        || r.contains("backend")
        || r.contains("frontend")
        || r.contains("engineer")
    {
        return AgentCategory::Implementers;
    }
    AgentCategory::Ops
}

pub fn is_manageable_pane_kind(kind: &str) -> bool {
    matches!(kind, "agent" | "factory-worker")
}

pub fn visible_pane<'a>(panes: &'a [PaneInfo], role: &str) -> Option<&'a PaneInfo> {
    panes
        .iter()
        .find(|p| p.role == role && (p.window == "main" || p.window == "tandem"))
}

pub fn agents_in<'a>(panes: &'a [PaneInfo], role: &str) -> Vec<&'a PaneInfo> {
    let mut agents: Vec<&'a PaneInfo> = panes
        .iter()
        .filter(|p| p.role == role && is_manageable_pane_kind(&p.kind))
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
    let base = if !pane.name.is_empty() {
        format!(
            "{} {} {}",
            pane.index,
            safe_label(&pane.name),
            safe_label(&pane.label)
        )
    } else {
        format!("{} {}", pane.index, safe_label(&pane.label))
    };
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
    let cli = if !pane.cli.is_empty() {
        Some(pane.cli.clone())
    } else if !pane.spec.is_empty() {
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
    if pane.kind == "factory-worker" {
        let mut parts: Vec<String> = Vec::new();
        let role_str = if pane.role.is_empty() {
            "worker".to_string()
        } else {
            safe_label(&pane.role)
        };
        parts.push(role_str);
        if !pane.stage.is_empty() {
            let st = safe_label(&pane.stage);
            if !parts.contains(&st) {
                parts.push(st);
            }
        }
        if !pane.run.is_empty() {
            parts.push(safe_label(&pane.run));
        }
        if !safe_cli.is_empty() {
            parts.push(safe_cli);
        }
        return format!("─ {active_marker}{}{dead_suffix} ─", parts.join(" · "));
    }
    let name_prefix = if !pane.name.is_empty() {
        format!("{} ({}-{})", safe_label(&pane.name), pane.role, pane.index)
    } else {
        format!("{}-{}", pane.role, pane.index)
    };
    format!("─ {active_marker}{name_prefix} · {safe_cli}{dead_suffix} ─",)
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
        .filter(|p| is_manageable_pane_kind(&p.kind))
        .or_else(|| {
            panes
                .iter()
                .find(|p| p.window == "main" && is_manageable_pane_kind(&p.kind))
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
