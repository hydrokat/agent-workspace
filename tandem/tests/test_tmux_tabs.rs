use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
use tandem::tmux::layout::{Launch, Layout};
use tandem::tmux::tabs::{
    AgentCategory, FORMAT, PaneInfo, agents_in, categorize_agent, is_manageable_pane_kind,
    neighbor, parse_panes, render_agent_title, render_title, tab_text, visible_pane,
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
        name: String::new(),
        idea: String::new(),
        ..PaneInfo::default()
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
    assert!(FORMAT.contains("@tandem_run"));
    assert!(FORMAT.contains("@tandem_job"));
    assert!(FORMAT.contains("@tandem_stage"));
    assert!(FORMAT.contains("@tandem_cli"));
    assert!(FORMAT.contains("@tandem_model"));
}

#[test]
fn test_parse_panes_reads_factory_worker_fields() {
    let row = "%12\tmain\tfactory-worker\tworker\t1\tclaude·opus\t0\t\tsess-1\t\t2.3.1\tdev\ti-123\tr-abc\tj-001\tdevelop\tclaude\tclaude-opus-4-7";
    let panes = parse_panes(row);
    assert_eq!(panes.len(), 1);
    let p = &panes[0];
    assert_eq!(p.kind, "factory-worker");
    assert_eq!(p.idea, "i-123");
    assert_eq!(p.run, "r-abc");
    assert_eq!(p.job, "j-001");
    assert_eq!(p.stage, "develop");
    assert_eq!(p.cli, "claude");
    assert_eq!(p.model, "claude-opus-4-7");
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

#[test]
fn test_render_agent_title_with_spaced_label_and_safe_label() {
    let mut p = pane(
        "%3",
        "main",
        "planner",
        1,
        "claude·opus",
        "agent",
        false,
        None,
    );
    p.name = "API worker - v2.1".to_string();
    let title = render_agent_title(&p);
    assert!(title.contains("API worker - v2.1"), "title was: {title}");
    let without_active_marker = title.replace("#{?pane_active,▶ ,}", "");
    assert!(
        !without_active_marker.contains('#'),
        "unexpected # in title: {title}"
    );

    let mut p_bad = pane(
        "%4",
        "main",
        "planner",
        2,
        "claude·opus",
        "agent",
        false,
        None,
    );
    p_bad.name = "bad#{pane_id}".to_string();
    let bad_title = render_agent_title(&p_bad);
    let bad_without_marker = bad_title.replace("#{?pane_active,▶ ,}", "");
    assert!(
        !bad_without_marker.contains('#') && !bad_without_marker.contains('{'),
        "safe_label must strip tmux format characters: {bad_title}"
    );
}

#[test]
fn test_agent_category_classifies_roles_and_stages() {
    assert_eq!(
        categorize_agent("orchestrator", ""),
        AgentCategory::Orchestrators
    );
    assert_eq!(categorize_agent("", "plan"), AgentCategory::Planners);
    assert_eq!(
        categorize_agent("business-analyst", ""),
        AgentCategory::Planners
    );
    assert_eq!(categorize_agent("", "create-tests"), AgentCategory::Testers);
    assert_eq!(
        categorize_agent("qa-engineer", "test"),
        AgentCategory::Testers
    );
    assert_eq!(
        categorize_agent("senior-backend-dev", "develop"),
        AgentCategory::Implementers
    );
    assert_eq!(
        categorize_agent("code-reviewer", "review"),
        AgentCategory::Reviewers
    );
    assert_eq!(
        categorize_agent("security-auditor", ""),
        AgentCategory::Reviewers
    );
    assert_eq!(
        categorize_agent("devops-engineer", "pr"),
        AgentCategory::Ops
    );
    assert_eq!(categorize_agent("demo-maker", "demo"), AgentCategory::Ops);
    assert_eq!(
        AgentCategory::all(),
        [
            AgentCategory::Orchestrators,
            AgentCategory::Planners,
            AgentCategory::Testers,
            AgentCategory::Implementers,
            AgentCategory::Reviewers,
            AgentCategory::Ops,
        ]
    );
}

#[test]
fn test_is_manageable_pane_kind_and_worker_title() {
    assert!(is_manageable_pane_kind("agent"));
    assert!(is_manageable_pane_kind("factory-worker"));
    assert!(!is_manageable_pane_kind("login"));
    assert!(!is_manageable_pane_kind("own"));
    assert!(!is_manageable_pane_kind("placeholder"));

    let mut worker = pane(
        "%77",
        "main",
        "backend",
        1,
        "develop·1",
        "factory-worker",
        false,
        None,
    );
    worker.run = "r-4h8k".to_string();
    worker.stage = "develop".to_string();
    worker.cli = "claude".to_string();
    let title = render_agent_title(&worker);
    assert!(title.contains("backend"), "missing role: {title}");
    assert!(title.contains("develop"), "missing stage: {title}");
    assert!(title.contains("r-4h8k"), "missing run: {title}");
    assert!(title.contains("claude"), "missing cli: {title}");

    let bar_title = render_title(&[worker.clone()], "main");
    assert_eq!(bar_title, title);
    assert_eq!(
        agents_in(&[worker], "backend")
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        vec!["%77"]
    );
}

#[test]
fn test_layout_show_pane_swaps_factory_worker_into_main_and_keeps_login_window() {
    let mut worker = pane(
        "%77",
        "fw-r-1-develop",
        "backend",
        1,
        "develop·1",
        "factory-worker",
        false,
        None,
    );
    worker.run = "r-1".to_string();
    worker.stage = "develop".to_string();
    worker.cli = "claude".to_string();

    let panes = vec![
        pane("%1", "main", "sidebar", 0, "", "own", false, None),
        pane(
            "%2",
            "main",
            "orchestrator",
            1,
            "claude",
            "agent",
            false,
            None,
        ),
        pane("%3", "main", "status", 0, "", "own", false, None),
        worker,
        pane(
            "%88",
            "login-claude",
            "login",
            0,
            "claude",
            "login",
            false,
            None,
        ),
    ];

    let list_out = panes
        .iter()
        .map(|p| {
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t0\t\t{}\t\t\t\t\t{}\t\t{}\t{}\t",
                p.id, p.window, p.kind, p.role, p.index, p.label, p.session, p.run, p.stage, p.cli
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut responses = HashMap::new();
    responses.insert(
        "list-panes",
        TmuxResult {
            code: 0,
            out: list_out,
            err: String::new(),
        },
    );
    responses.insert(
        "display-message",
        TmuxResult {
            code: 0,
            out: "%2\t160\t40\n".to_string(),
            err: String::new(),
        },
    );

    let runner = Arc::new(MockRunner::new(responses));
    let client = TmuxClient::with_runner("test-sock", Some(PathBuf::from("/conf")), runner.clone());
    let layout = Layout::new(&client);

    layout.select_agent("%77", false).unwrap();
    let worker_calls = runner.calls();
    assert!(
        worker_calls.iter().any(|c| {
            c.windows(5)
                .any(|w| w == ["swap-pane", "-d", "-s", "%77", "-t"])
        }),
        "factory-worker must swap-pane into main: {worker_calls:?}"
    );
    assert!(
        !worker_calls
            .iter()
            .any(|c| c.contains(&"select-window".to_string())),
        "factory-worker must never select-window: {worker_calls:?}"
    );

    layout.select_agent("%77", true).unwrap();
    let toggle_calls = runner.calls();
    assert!(
        toggle_calls
            .iter()
            .any(|c| c.contains(&"join-pane".to_string())),
        "grid toggle on factory-worker must join-pane inside main: {toggle_calls:?}"
    );

    layout.show_pane_with(&panes, "%88").unwrap();
    let login_calls = runner.calls();
    assert!(
        login_calls
            .iter()
            .any(|c| c.contains(&"select-window".to_string()) && c.contains(&"%88".to_string())),
        "login pane must still switch windows"
    );
}

#[test]
fn test_layout_prepare_pane_removal_swaps_fallback_or_respawns_placeholder() {
    let worker_in_main = pane(
        "%77",
        "main",
        "backend",
        1,
        "develop·1",
        "factory-worker",
        false,
        None,
    );
    let stashed_orch = pane(
        "%2",
        "stash-orchestrator-1",
        "orchestrator",
        1,
        "claude",
        "agent",
        false,
        None,
    );

    let with_fallback = [
        "%1\tmain\town\tsidebar\t0\t\t0\t\t",
        &format!(
            "{}\t{}\t{}\t{}\t1\t{}\t0\t\t",
            worker_in_main.id,
            worker_in_main.window,
            worker_in_main.kind,
            worker_in_main.role,
            worker_in_main.label
        ),
        "%3\tmain\town\tstatus\t0\t\t0\t\t",
        &format!(
            "{}\t{}\t{}\t{}\t1\t{}\t0\t\tsess-orch",
            stashed_orch.id,
            stashed_orch.window,
            stashed_orch.kind,
            stashed_orch.role,
            stashed_orch.label
        ),
    ]
    .join("\n");

    let mut responses = HashMap::new();
    responses.insert(
        "list-panes",
        TmuxResult {
            code: 0,
            out: with_fallback,
            err: String::new(),
        },
    );
    responses.insert(
        "display-message",
        TmuxResult {
            code: 0,
            out: "%77\t160\t40\n".to_string(),
            err: String::new(),
        },
    );
    let runner = Arc::new(MockRunner::new(responses));
    let client = TmuxClient::with_runner("test-sock", Some(PathBuf::from("/conf")), runner.clone());
    let layout = Layout::new(&client);
    let ph = Launch::new(
        vec!["sleep".to_string(), "1".to_string()],
        "orchestrator",
        0,
        "placeholder",
    );

    let replaced_in_place = layout.prepare_pane_removal("%77", &ph).unwrap();
    assert!(!replaced_in_place);
    let calls = runner.calls();
    assert!(
        calls.iter().any(|c| c
            .windows(5)
            .any(|w| w == ["swap-pane", "-d", "-s", "%2", "-t"])),
        "expected stashed agent %2 swapped into main before kill-pane: {calls:?}"
    );

    let solo_in_main = [
        "%1\tmain\town\tsidebar\t0\t\t0\t\t",
        "%77\tmain\tfactory-worker\tbackend\t1\tdevelop·1\t0\t\t",
        "%3\tmain\town\tstatus\t0\t\t0\t\t",
    ]
    .join("\n");
    let mut solo_resp = HashMap::new();
    solo_resp.insert(
        "list-panes",
        TmuxResult {
            code: 0,
            out: solo_in_main,
            err: String::new(),
        },
    );
    solo_resp.insert(
        "display-message",
        TmuxResult {
            code: 0,
            out: "%77\t160\t40\n".to_string(),
            err: String::new(),
        },
    );
    let solo_runner = Arc::new(MockRunner::new(solo_resp));
    let solo_client = TmuxClient::with_runner(
        "test-sock",
        Some(PathBuf::from("/conf")),
        solo_runner.clone(),
    );
    let solo_layout = Layout::new(&solo_client);
    let solo_replaced = solo_layout.prepare_pane_removal("%77", &ph).unwrap();
    assert!(solo_replaced);
    assert!(
        solo_runner
            .calls()
            .iter()
            .any(|c| c.contains(&"respawn-pane".to_string())),
        "expected respawn-pane placeholder when solo worker closes in main"
    );
}
