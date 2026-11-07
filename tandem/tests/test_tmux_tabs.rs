use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
use tandem::tmux::layout::{Launch, Layout};
use tandem::tmux::tabs::{
    FORMAT, PaneInfo, agents_in, neighbor, parse_panes, render_title, tab_text, visible_pane,
};

#[allow(clippy::too_many_arguments)]
fn pane(
    pid: &str,
    window: &str,
    role: &str,
    index: usize,
    label: &str,
    kind: &str,
    dead: bool,
    status: Option<i32>,
) -> PaneInfo {
    PaneInfo {
        id: pid.to_string(),
        window: window.to_string(),
        kind: kind.to_string(),
        role: role.to_string(),
        index,
        label: label.to_string(),
        dead,
        dead_status: status,
        session: String::new(),
        spec: String::new(),
        cli_version: String::new(),
    }
}

fn sample_panes() -> Vec<PaneInfo> {
    vec![
        pane(
            "%1",
            "main",
            "planner",
            1,
            "claude·opus",
            "agent",
            false,
            None,
        ),
        pane(
            "%2",
            "main",
            "implementer",
            1,
            "claude·sonnet",
            "agent",
            false,
            None,
        ),
        pane("%3", "main", "status", 0, "", "own", false, None),
        pane(
            "%4",
            "w-planner-2",
            "planner",
            2,
            "agy·argon",
            "agent",
            false,
            None,
        ),
        pane(
            "%5",
            "w-planner-3",
            "planner",
            3,
            "agy·flash",
            "agent",
            true,
            Some(1),
        ),
    ]
}

#[test]
fn test_parse_panes_round_trip() {
    let panes = sample_panes();
    let text = panes
        .iter()
        .map(|p| {
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                p.id,
                p.window,
                p.kind,
                p.role,
                p.index,
                p.label,
                if p.dead { "1" } else { "0" },
                p.dead_status.map(|s| s.to_string()).unwrap_or_default(),
                p.session
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(parse_panes(&text), panes);
}

#[test]
fn test_parse_panes_skips_garbage_and_untagged() {
    assert_eq!(parse_panes("garbage\n%9\tmain\t\t\t\t\t0\t\n\n"), vec![]);
}

#[test]
fn test_format_uses_user_options() {
    assert!(FORMAT.contains("@tandem_role"));
    assert!(FORMAT.contains("pane_dead_status"));
}

#[test]
fn test_visible_pane_is_the_one_in_main() {
    let panes = sample_panes();
    assert_eq!(visible_pane(&panes, "planner"), Some(&panes[0]));
    assert_eq!(visible_pane(&panes, "implementer"), Some(&panes[1]));
}

#[test]
fn test_agents_in_sorted_and_excludes_placeholders() {
    let mut mixed = sample_panes();
    mixed.push(pane("%6", "main", "x", 0, "", "own", false, None));
    let ids: Vec<&str> = agents_in(&mixed, "planner")
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(ids, vec!["%1", "%4", "%5"]);
}

#[test]
fn test_neighbor_wraps() {
    let panes = sample_panes();
    assert_eq!(neighbor(&panes, "planner", 1).unwrap().id, "%4");
    assert_eq!(neighbor(&panes, "planner", -1).unwrap().id, "%5");
    assert_eq!(neighbor(&panes, "implementer", 1).unwrap().id, "%2");
}

#[test]
fn test_neighbor_none_when_column_empty() {
    let panes = sample_panes();
    assert!(neighbor(&panes, "nobody", 1).is_none());
}

#[test]
fn test_title_marks_visible_and_exited() {
    let panes = sample_panes();
    let title = render_title(&panes, "planner");
    assert_eq!(title, "─ #{?pane_active,▶ ,}planner-1 · claude ─");
    let exited = vec![pane(
        "%1",
        "main",
        "planner",
        1,
        "claude·opus",
        "agent",
        true,
        Some(1),
    )];
    let dead_title = render_title(&exited, "planner");
    assert_eq!(
        dead_title,
        "─ #{?pane_active,▶ ,}planner-1 · claude (exited 1) ─"
    );
}

#[test]
fn test_title_empty_column() {
    assert!(render_title(&[], "planner").contains("No planners yet"));
}

#[test]
fn test_title_escapes_tmux_format_characters() {
    let evil = vec![pane(
        "%1",
        "main",
        "planner",
        1,
        "a#{x}b,c}",
        "agent",
        false,
        None,
    )];
    let title = render_title(&evil, "planner");
    let stripped = title.replace("#{?pane_active,▶ ,}", "");
    assert!(!stripped.contains("#{x}"));
    assert!(title.contains("a"));
}

#[test]
fn test_tab_text_shows_index_and_label() {
    let p = pane(
        "%1",
        "main",
        "planner",
        2,
        "claude·opus",
        "agent",
        false,
        None,
    );
    assert_eq!(tab_text(&p), "2 claude·opus");
}

#[test]
fn test_tab_text_marks_exited_agents() {
    let dead = pane("%1", "main", "planner", 3, "agy", "agent", true, Some(7));
    assert_eq!(tab_text(&dead), "⚠3 agy(exited 7)");
    let unknown = pane("%1", "main", "planner", 3, "agy", "agent", true, None);
    assert_eq!(tab_text(&unknown), "⚠3 agy(exited ?)");
}

#[test]
fn test_tab_text_replaces_unsafe_characters() {
    let p = pane("%1", "main", "planner", 1, "a#{x}b", "agent", false, None);
    assert!(!tab_text(&p).contains('#'));
}

#[test]
fn test_render_title_shows_agent_title() {
    let panes = vec![
        pane("%1", "main", "planner", 1, "claude", "agent", false, None),
        pane("%2", "stash", "planner", 2, "agy", "agent", false, None),
    ];
    let title = render_title(&panes, "planner");
    assert_eq!(title, "─ #{?pane_active,▶ ,}planner-1 · claude ─");
}

struct MockRunner {
    calls: Mutex<Vec<Vec<String>>>,
    responses: HashMap<String, TmuxResult>,
}

impl MockRunner {
    fn new(responses: HashMap<&str, TmuxResult>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responses: responses
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl TmuxRunner for MockRunner {
    fn run(&self, argv: &[String]) -> TmuxResult {
        self.calls.lock().unwrap().push(argv.to_vec());
        let sub = argv
            .iter()
            .position(|a| a == "-f")
            .and_then(|idx| argv.get(idx + 2))
            .map(|s| s.as_str())
            .unwrap_or("");
        if let Some(res) = self.responses.get(sub) {
            res.clone()
        } else {
            TmuxResult {
                code: 0,
                out: "%1\n".to_string(),
                err: String::new(),
            }
        }
    }
}

#[test]
fn test_layout_build_sequence() {
    let runner = Arc::new(MockRunner::new(HashMap::new()));
    let client = TmuxClient::with_runner("test-sock", Some(PathBuf::from("/conf")), runner.clone());
    let layout = Layout::new(&client);

    let left = Launch::new(vec!["claude".to_string()], "planner", 1, "claude·opus");
    let right = Launch::new(vec!["agy".to_string()], "implementer", 1, "agy·sonnet");
    let status = Launch::new(vec!["tandem".to_string()], "status", 0, "status");

    let res = layout.build(200, 50, &left, &right, &status);
    assert!(res.is_ok());

    let calls = runner.calls();
    assert!(calls.iter().any(|c| c.contains(&"new-session".to_string())));
    assert!(
        calls
            .iter()
            .any(|c| c.contains(&"split-window".to_string()))
    );
    assert!(calls.iter().any(|c| c.contains(&"set-option".to_string())));
}

#[test]
fn test_layout_add_agent_and_swap() {
    let mut responses = HashMap::new();
    responses.insert(
        "new-window",
        TmuxResult {
            code: 0,
            out: "%9\n".to_string(),
            err: String::new(),
        },
    );
    responses.insert(
        "list-panes",
        TmuxResult {
            code: 0,
            out: "%1\tmain\tagent\tplanner\t1\tclaude\t0\t\ts-1\n".to_string(),
            err: String::new(),
        },
    );
    let runner = Arc::new(MockRunner::new(responses));
    let client = TmuxClient::with_runner("test-sock", Some(PathBuf::from("/conf")), runner.clone());
    let layout = Layout::new(&client);

    let launch = Launch::new(vec!["claude".to_string()], "planner", 2, "claude·sonnet");
    let res = layout.add_agent(&launch, true);
    assert!(res.is_ok());

    let calls = runner.calls();
    assert!(calls.iter().any(|c| c.contains(&"new-window".to_string())));
    assert!(calls.iter().any(|c| c.contains(&"swap-pane".to_string())));
}
