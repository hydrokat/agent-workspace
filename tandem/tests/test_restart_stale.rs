use std::collections::HashMap;

use tandem::tmux::tabs::PaneInfo;
use tandem::updater::{
    StaleAgent, choose_restarts, default_selection, describe_stale, plan, stale_agents,
};

fn make_pane(
    pid: &str,
    role: &str,
    index: usize,
    cli: &str,
    launched: &str,
    kind: &str,
    session: &str,
) -> PaneInfo {
    let spec =
        format!(r#"{{"cli": "{cli}", "model": "opus", "skip_permissions": false, "cwd": null}}"#);
    let sess = if session.is_empty() {
        format!("{role}-{index}")
    } else {
        session.to_string()
    };
    let label = format!("{cli}·opus");
    let mut p = PaneInfo::new(pid, "main", kind, role, index, &label, false, None);
    p.session = sess;
    p.spec = spec;
    p.cli_version = launched.to_string();
    p
}

#[test]
fn test_stale_means_behind_or_unknown_and_skips_current_and_unprobeable_clis() {
    let panes = vec![
        make_pane("%1", "planner", 1, "claude", "2.1.305", "agent", ""),
        make_pane("%2", "planner", 2, "claude", "2.1.310", "agent", ""),
        make_pane("%3", "implementer", 1, "agy", "", "agent", ""),
        make_pane("%4", "implementer", 2, "agy", "1.4.0", "agent", ""),
        make_pane("%5", "status", 0, "status", "", "own", ""),
        make_pane("%6", "implementer", 3, "codex", "0.1.0", "agent", ""),
    ];

    let installed_map = HashMap::from([
        ("claude".to_string(), "2.1.310".to_string()),
        ("agy".to_string(), "1.4.0".to_string()),
    ]);
    let installed = |cli: &str| installed_map.get(cli).cloned().unwrap_or_default();

    let found = stale_agents(&panes, installed, |_| true, &HashMap::new());

    let summary: Vec<(String, String, String)> = found
        .iter()
        .map(|s| (s.pane_id.clone(), s.launched.clone(), s.installed.clone()))
        .collect();

    assert_eq!(
        summary,
        vec![
            (
                "%1".to_string(),
                "2.1.305".to_string(),
                "2.1.310".to_string()
            ),
            ("%3".to_string(), "".to_string(), "1.4.0".to_string()),
        ]
    );
    assert_eq!(found[0].role, "planner");
    assert_eq!(found[0].cli, "claude");
    assert_eq!(found[0].model, "opus");
}

#[test]
fn test_flags_agents_that_cannot_resume_or_are_busy() {
    let panes = vec![
        make_pane("%1", "planner", 1, "claude", "2.1.305", "agent", ""),
        make_pane("%2", "planner", 2, "claude", "2.1.305", "agent", ""),
        make_pane("%3", "implementer", 1, "claude", "2.1.305", "agent", ""),
    ];

    let installed_map = HashMap::from([("claude".to_string(), "2.1.310".to_string())]);
    let installed = |cli: &str| installed_map.get(cli).cloned().unwrap_or_default();
    let busy = HashMap::from([("implementer-1".to_string(), "p-004 t-002".to_string())]);

    let found = stale_agents(&panes, installed, |p| p.id != "%2", &busy);

    let flags: HashMap<String, (bool, bool, String)> = found
        .iter()
        .map(|s| (s.pane_id.clone(), (s.resumable, s.busy, s.claim.clone())))
        .collect();

    assert_eq!(
        flags,
        HashMap::from([
            ("%1".to_string(), (true, false, "".to_string())),
            ("%2".to_string(), (false, false, "".to_string())),
            ("%3".to_string(), (true, true, "p-004 t-002".to_string())),
        ])
    );

    let defaults = default_selection(&found);
    assert_eq!(defaults, ["%1".to_string()].into_iter().collect());
}

#[test]
fn test_nothing_stale_when_all_are_current() {
    let panes = vec![make_pane(
        "%1", "planner", 1, "claude", "2.1.310", "agent", "",
    )];
    let installed = |cli: &str| {
        if cli == "claude" {
            "2.1.310".to_string()
        } else {
            String::new()
        }
    };
    let found = stale_agents(&panes, installed, |_| true, &HashMap::new());
    assert!(found.is_empty());
}

fn make_stale_test_agents() -> Vec<StaleAgent> {
    let panes = vec![
        make_pane("%1", "planner", 1, "claude", "2.1.305", "agent", ""),
        make_pane("%2", "implementer", 1, "claude", "2.1.305", "agent", ""),
        make_pane("%3", "implementer", 2, "claude", "2.1.305", "agent", ""),
    ];
    let installed = |_: &str| "2.1.310".to_string();
    let busy = HashMap::from([("implementer-1".to_string(), "p-004 t-002".to_string())]);
    stale_agents(&panes, installed, |p| p.id != "%3", &busy)
}

fn choose_helper(answers: Vec<&str>) -> Vec<String> {
    let mut feed = answers.into_iter();
    let mut ask = |_prompt: &str| feed.next().map(|s| s.to_string());
    let mut lines = Vec::new();
    let mut out = |line: &str| lines.push(line.to_string());
    let chosen = choose_restarts(&make_stale_test_agents(), &mut ask, &mut out);
    assert!(lines.iter().any(|l| l.contains("still run an older CLI")));
    chosen.into_iter().map(|s| s.pane_id).collect()
}

#[test]
fn test_enter_restarts_the_safe_defaults_only() {
    assert_eq!(choose_helper(vec![""]), vec!["%1"]);
}

#[test]
fn test_all_numbers_and_none() {
    let all = choose_helper(vec!["a", ""]);
    assert_eq!(all, vec!["%1", "%2", "%3"]);

    let toggled = choose_helper(vec!["2 3", ""]);
    assert_eq!(toggled, vec!["%1", "%2", "%3"]);

    let deselect = choose_helper(vec!["1", ""]);
    assert!(deselect.is_empty());

    let none = choose_helper(vec!["n"]);
    assert!(none.is_empty());

    let all_then_none = choose_helper(vec!["a", "n"]);
    assert!(all_then_none.is_empty());
}

#[test]
fn test_ignores_junk_and_treats_end_of_input_as_none() {
    let junk = choose_helper(vec!["9 x -1", ""]);
    assert_eq!(junk, vec!["%1"]);

    let eof = choose_helper(vec![]);
    assert!(eof.is_empty());
}

#[test]
fn test_the_update_plan_mentions_the_ai_cli_step() {
    let dir = std::env::temp_dir();
    let text = plan(&dir, "o/r", Some(true)).join("\n");
    assert!(text.contains("claude update"));
    assert!(text.contains("agents keep running"));
}

#[test]
fn test_describe_stale_busy_and_not_resumable() {
    let busy_agent = StaleAgent {
        pane_id: "%2".to_string(),
        role: "implementer".to_string(),
        index: 1,
        cli: "claude".to_string(),
        model: "sonnet".to_string(),
        launched: "2.1.305".to_string(),
        installed: "2.1.310".to_string(),
        resumable: true,
        busy: true,
        claim: "p-004 t-002".to_string(),
    };
    let d = describe_stale(&busy_agent);
    assert!(d.contains("busy (claim p-004 t-002)"));

    let not_resumable = StaleAgent {
        pane_id: "%3".to_string(),
        role: "implementer".to_string(),
        index: 2,
        cli: "agy".to_string(),
        model: "flash".to_string(),
        launched: "1.3.5".to_string(),
        installed: "1.3.6".to_string(),
        resumable: false,
        busy: false,
        claim: String::new(),
    };
    let d2 = describe_stale(&not_resumable);
    assert!(d2.contains("would start a new chat"));
}

#[test]
fn test_agy_restarts_advised_note() {
    let panes = vec![make_pane(
        "%1",
        "implementer",
        1,
        "agy",
        "1.0.0",
        "agent",
        "",
    )];
    let installed = |_: &str| "1.1.0".to_string();
    let stale = stale_agents(&panes, installed, |_| true, &HashMap::new());

    let mut lines = Vec::new();
    let mut out = |line: &str| lines.push(line.to_string());
    let mut ask = |_prompt: &str| Some("n".to_string());
    choose_restarts(&stale, &mut ask, &mut out);

    assert!(
        lines
            .iter()
            .any(|l| l.contains("agy: the update may replace its binary"))
    );
}
