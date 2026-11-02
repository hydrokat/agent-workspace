mod common;

use common::parity::load_fixture_value;
use tandem::coordination::schema::{Claim, Session};
use tandem::snapshot::StatusSnapshot;
use tandem::sources::conflicts::Conflict;
use tandem::sources::specs::{Phase, Task};
use tandem::status::render::{
    age, bar, conflicts_text, fit, in_progress_text, phases_text, session_row, summary_line,
};
use unicode_width::UnicodeWidthStr;

#[test]
fn test_render_parity_fixture() {
    let fixture = load_fixture_value("render");

    let age_cases = fixture["age"].as_object().unwrap();
    assert_eq!(age(12.0), age_cases["12s"].as_str().unwrap());
    assert_eq!(age(840.0), age_cases["840s"].as_str().unwrap());
    assert_eq!(age(7200.0), age_cases["7200s"].as_str().unwrap());

    let bar_cases = fixture["bar"].as_object().unwrap();
    assert_eq!(bar(0, 0, 4), bar_cases["0_of_0_w4"].as_str().unwrap());
    assert_eq!(bar(1, 2, 4), bar_cases["1_of_2_w4"].as_str().unwrap());
    assert_eq!(bar(3, 3, 6), bar_cases["3_of_3_w6"].as_str().unwrap());

    let fit_cases = fixture["fit"].as_object().unwrap();
    for (text, widths_obj) in fit_cases {
        let widths = widths_obj.as_object().unwrap();
        for (w_str, expected) in widths {
            let width: usize = w_str.parse().unwrap();
            let result = fit(text, width);
            assert_eq!(result, expected.as_str().unwrap(), "fit({text:?}, {width})");
        }
    }

    let empty_snap = StatusSnapshot {
        phases: Vec::new(),
        entries: Vec::new(),
        sessions: Vec::new(),
        claims: Vec::new(),
        queries: Vec::new(),
        conflicts: Vec::new(),
        codebases: Vec::new(),
        workspace_git: None,
        codebase_git: std::collections::HashMap::new(),
        coord_exists: true,
        now: 10_000.0,
        sprint: None,
        ongoing_specs: Vec::new(),
        ongoing_tasks: Vec::new(),
    };

    let summary_cases = fixture["summary_line"].as_object().unwrap();
    assert_eq!(
        summary_line(&empty_snap),
        summary_cases["empty"].as_str().unwrap()
    );

    let active_snap = StatusSnapshot {
        phases: vec![Phase::new(
            "p-001-core",
            vec![
                Task::new("t-001", "Initial setup", "Done"),
                Task::new("t-002", "Core logic", "In Progress"),
            ],
        )],
        entries: Vec::new(),
        sessions: vec![Session {
            id: "implementer-1".to_string(),
            cli: "claude".to_string(),
            role: "implementer".to_string(),
            model: "sonnet".to_string(),
            pane: "%1".to_string(),
            cwd: "/tmp".to_string(),
            branch: "feat/core".to_string(),
            worktree: "/tmp".to_string(),
            task: "p-001 t-002".to_string(),
            heartbeat: 10_000.0 - 15.0,
        }],
        claims: vec![Claim {
            phase: "p-001-core".to_string(),
            task: "t-002".to_string(),
            session: "implementer-1".to_string(),
            branch: "feat/core".to_string(),
            worktree: "/tmp".to_string(),
            paths: vec!["src/core.rs".to_string()],
            heartbeat: 10_000.0 - 15.0,
        }],
        queries: Vec::new(),
        conflicts: vec![Conflict::new("t-002", "t-003", "src/core.rs")],
        codebases: Vec::new(),
        workspace_git: None,
        codebase_git: std::collections::HashMap::new(),
        coord_exists: true,
        now: 10_000.0,
        sprint: None,
        ongoing_specs: Vec::new(),
        ongoing_tasks: Vec::new(),
    };

    assert_eq!(
        summary_line(&active_snap),
        summary_cases["active"].as_str().unwrap()
    );

    let session_cases = fixture["session_row"].as_object().unwrap();
    assert_eq!(
        session_row(&active_snap, 0),
        session_cases["active"].as_str().unwrap()
    );

    let in_progress_cases = fixture["in_progress_text"].as_object().unwrap();
    assert_eq!(
        in_progress_text(&empty_snap),
        in_progress_cases["empty"].as_str().unwrap()
    );
    assert_eq!(
        in_progress_text(&active_snap),
        in_progress_cases["active"].as_str().unwrap()
    );

    let phases_cases = fixture["phases_text"].as_object().unwrap();
    assert_eq!(
        phases_text(&empty_snap),
        phases_cases["empty"].as_str().unwrap()
    );
    assert_eq!(
        phases_text(&active_snap),
        phases_cases["active"].as_str().unwrap()
    );

    let conflicts_cases = fixture["conflicts_text"].as_object().unwrap();
    assert_eq!(
        conflicts_text(&empty_snap),
        conflicts_cases["empty"].as_str().unwrap()
    );
    assert_eq!(
        conflicts_text(&active_snap),
        conflicts_cases["active"].as_str().unwrap()
    );
}

#[test]
fn test_fit_cjk_and_emoji_widths() {
    let cjk_sample = "这是一个很长的中文字符串用于测试宽度截断效果确保无溢出";
    let emoji_sample = "Feature 🚀 with multiple 🔥 emojis and symbols ✨ for testing";

    for width in [40, 80, 120] {
        let fitted_cjk = fit(cjk_sample, width);
        let cjk_w = UnicodeWidthStr::width(fitted_cjk.as_str());
        assert!(
            cjk_w <= width,
            "CJK width {cjk_w} exceeded {width} for {fitted_cjk}"
        );

        let fitted_emoji = fit(emoji_sample, width);
        let emoji_w = UnicodeWidthStr::width(fitted_emoji.as_str());
        assert!(
            emoji_w <= width,
            "Emoji width {emoji_w} exceeded {width} for {fitted_emoji}"
        );
    }
}
