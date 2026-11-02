mod common;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use tandem::snapshot::{SprintPhaseView, SprintView, StatusSnapshot};
use tandem::sources::specs::Phase;
use tandem::status::sprint_tab;
use tandem::ui::scroll::ScrollView;

fn render_lines_to_text(lines: &[Line], width: u16, height: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    let mut scroll = ScrollView::new();
    scroll.render_lines(lines, Rect::new(0, 0, width, height), &mut buf);
    buffer_text(&buf)
}

fn buffer_text(buf: &Buffer) -> String {
    let mut s = String::new();
    let area = buf.area;
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell((x, y)) {
                s.push_str(cell.symbol());
            }
        }
        s.push('\n');
    }
    s
}

fn timestamp_for_date(date_str: &str) -> f64 {
    let d = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").unwrap();
    d.and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp() as f64
}

#[test]
fn test_empty_state_when_no_active_sprint() {
    let snapshot = StatusSnapshot::default();
    let lines = sprint_tab::lines(&snapshot, 80);
    assert_eq!(lines.len(), 1);
    let text = render_lines_to_text(&lines, 80, 5);
    assert!(text.contains("No active sprint. Plan one with /pm-new-sprint."));
}

#[test]
fn test_active_sprint_header_and_progress() {
    let sprint = SprintView {
        id: "s-003-tandem-rust-cutover".to_string(),
        title: "tandem Rust cutover and release".to_string(),
        status: "open".to_string(),
        start_date: "2026-11-02".to_string(),
        end_date: "2026-11-08".to_string(),
        capacity_days: 5.0,
        committed_days: 4.5,
        overloaded: false,
        done: 4,
        total: 14,
        must_remaining: 6,
        phases: vec![
            SprintPhaseView {
                name: "p-012-agent-workspace-package".to_string(),
                moscow: "Must Have".to_string(),
                effort: 3.0,
                done: 7,
                total: 9,
                archived: false,
            },
            SprintPhaseView {
                name: "p-013-agent-workspace-package".to_string(),
                moscow: "Should Have".to_string(),
                effort: 1.5,
                done: 2,
                total: 5,
                archived: false,
            },
        ],
    };

    let p1 = Phase {
        name: "p-012-agent-workspace-package".to_string(),
        tasks: Vec::new(),
        title: String::new(),
        branch: String::new(),
        target: String::new(),
        warning: String::new(),
        archived: false,
    };
    let p2 = Phase {
        name: "p-013-agent-workspace-package".to_string(),
        tasks: Vec::new(),
        title: String::new(),
        branch: String::new(),
        target: String::new(),
        warning: String::new(),
        archived: false,
    };

    let snapshot = StatusSnapshot {
        sprint: Some(sprint),
        phases: vec![p1, p2],
        now: timestamp_for_date("2026-11-05"),
        ..Default::default()
    };

    let lines = sprint_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);

    assert!(text.contains("s-003-tandem-rust-cutover"));
    assert!(text.contains("open"));
    assert!(text.contains("Nov 2 → Nov 8"));
    assert!(text.contains("3 days left"));
    assert!(text.contains("capacity"));
    assert!(text.contains("4.5 / 5.0 days"));
    assert!(!text.contains("overloaded"));
    assert!(text.contains("progress"));
    assert!(text.contains("4 / 14 tasks"));
    assert!(text.contains("must-have left: 6"));
    assert!(text.contains("p-012-agent-workspace-package"));
    assert!(text.contains("Must Have"));
    assert!(text.contains("3.0d"));
    assert!(text.contains("7/9"));
    assert!(text.contains("p-013-agent-workspace-package"));
    assert!(text.contains("Should Have"));
    assert!(text.contains("1.5d"));
    assert!(text.contains("2/5"));
}

#[test]
fn test_overloaded_sprint_shows_overloaded_flag() {
    let sprint = SprintView {
        id: "s-004-big".to_string(),
        title: "Large sprint".to_string(),
        status: "open".to_string(),
        start_date: "2026-11-09".to_string(),
        end_date: "2026-11-13".to_string(),
        capacity_days: 5.0,
        committed_days: 7.5,
        overloaded: true,
        done: 0,
        total: 10,
        must_remaining: 10,
        phases: vec![],
    };

    let snapshot = StatusSnapshot {
        sprint: Some(sprint),
        now: timestamp_for_date("2026-11-09"),
        ..Default::default()
    };

    let lines = sprint_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);
    assert!(text.contains("overloaded"));
    assert!(text.contains("7.5 / 5.0 days"));
}

#[test]
fn test_sprint_ended_status() {
    let sprint = SprintView {
        id: "s-001-done".to_string(),
        title: "Closed sprint".to_string(),
        status: "closed".to_string(),
        start_date: "2026-10-01".to_string(),
        end_date: "2026-10-07".to_string(),
        capacity_days: 5.0,
        committed_days: 5.0,
        overloaded: false,
        done: 10,
        total: 10,
        must_remaining: 0,
        phases: vec![],
    };

    let snapshot = StatusSnapshot {
        sprint: Some(sprint),
        now: timestamp_for_date("2026-10-15"),
        ..Default::default()
    };

    let lines = sprint_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);
    assert!(text.contains("ended"));
}

#[test]
fn test_phase_rows_sorted_in_moscow_order() {
    let sprint = SprintView {
        id: "s-005-sort".to_string(),
        title: "Sorting sprint".to_string(),
        status: "open".to_string(),
        start_date: "2026-11-02".to_string(),
        end_date: "2026-11-08".to_string(),
        capacity_days: 5.0,
        committed_days: 4.0,
        overloaded: false,
        done: 0,
        total: 6,
        must_remaining: 2,
        phases: vec![
            SprintPhaseView {
                name: "p-could".to_string(),
                moscow: "Could Have".to_string(),
                effort: 0.5,
                done: 0,
                total: 2,
                archived: false,
            },
            SprintPhaseView {
                name: "p-must".to_string(),
                moscow: "Must Have".to_string(),
                effort: 2.0,
                done: 0,
                total: 2,
                archived: false,
            },
            SprintPhaseView {
                name: "p-should".to_string(),
                moscow: "Should Have".to_string(),
                effort: 1.5,
                done: 0,
                total: 2,
                archived: false,
            },
        ],
    };

    let p_must = Phase {
        name: "p-must".to_string(),
        tasks: Vec::new(),
        title: String::new(),
        branch: String::new(),
        target: String::new(),
        warning: String::new(),
        archived: false,
    };
    let p_should = Phase {
        name: "p-should".to_string(),
        tasks: Vec::new(),
        title: String::new(),
        branch: String::new(),
        target: String::new(),
        warning: String::new(),
        archived: false,
    };
    let p_could = Phase {
        name: "p-could".to_string(),
        tasks: Vec::new(),
        title: String::new(),
        branch: String::new(),
        target: String::new(),
        warning: String::new(),
        archived: false,
    };

    let snapshot = StatusSnapshot {
        sprint: Some(sprint),
        phases: vec![p_could, p_must, p_should],
        now: timestamp_for_date("2026-11-03"),
        ..Default::default()
    };

    let lines = sprint_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);

    let idx_must = text.find("p-must").expect("p-must should be present");
    let idx_should = text.find("p-should").expect("p-should should be present");
    let idx_could = text.find("p-could").expect("p-could should be present");

    assert!(
        idx_must < idx_should,
        "Must Have should precede Should Have"
    );
    assert!(
        idx_should < idx_could,
        "Should Have should precede Could Have"
    );
}

#[test]
fn test_missing_phase_directory_shows_warning() {
    let sprint = SprintView {
        id: "s-006-missing".to_string(),
        title: "Missing phase sprint".to_string(),
        status: "open".to_string(),
        start_date: "2026-11-02".to_string(),
        end_date: "2026-11-08".to_string(),
        capacity_days: 5.0,
        committed_days: 3.5,
        overloaded: false,
        done: 0,
        total: 2,
        must_remaining: 0,
        phases: vec![SprintPhaseView {
            name: "p-099-gone".to_string(),
            moscow: "Could Have".to_string(),
            effort: 0.5,
            done: 0,
            total: 0,
            archived: false,
        }],
    };

    let snapshot = StatusSnapshot {
        sprint: Some(sprint),
        phases: Vec::new(),
        now: timestamp_for_date("2026-11-03"),
        ..Default::default()
    };

    let lines = sprint_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);

    assert!(text.contains("p-099-gone"));
    assert!(text.contains("(no specs/ directory)"));
}

#[test]
fn test_untrusted_text_is_sanitized() {
    let sprint = SprintView {
        id: "s-007-escapes\x1b[31m".to_string(),
        title: "Title\x1b[0mInjected".to_string(),
        status: "open".to_string(),
        start_date: "2026-11-02".to_string(),
        end_date: "2026-11-08".to_string(),
        capacity_days: 5.0,
        committed_days: 1.0,
        overloaded: false,
        done: 0,
        total: 1,
        must_remaining: 0,
        phases: vec![SprintPhaseView {
            name: "p-phase\x1b[32m-test".to_string(),
            moscow: "Must Have".to_string(),
            effort: 1.0,
            done: 0,
            total: 1,
            archived: false,
        }],
    };

    let snapshot = StatusSnapshot {
        sprint: Some(sprint),
        phases: Vec::new(),
        now: timestamp_for_date("2026-11-03"),
        ..Default::default()
    };

    let lines = sprint_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);

    assert!(!text.contains('\x1b'));
    assert!(text.contains("s-007-escapes"));
    assert!(text.contains("TitleInjected"));
    assert!(text.contains("p-phase-test"));
}

#[test]
fn test_narrow_width_responsive_shrinks_bars_and_truncates() {
    let sprint = SprintView {
        id: "s-008-narrow".to_string(),
        title: "Narrow sprint".to_string(),
        status: "open".to_string(),
        start_date: "2026-11-02".to_string(),
        end_date: "2026-11-08".to_string(),
        capacity_days: 5.0,
        committed_days: 2.5,
        overloaded: false,
        done: 2,
        total: 4,
        must_remaining: 2,
        phases: vec![SprintPhaseView {
            name: "p-extremely-long-phase-name-that-cannot-fit-on-narrow-screen".to_string(),
            moscow: "Must Have".to_string(),
            effort: 2.5,
            done: 2,
            total: 4,
            archived: false,
        }],
    };

    let snapshot = StatusSnapshot {
        sprint: Some(sprint),
        phases: vec![Phase {
            name: "p-extremely-long-phase-name-that-cannot-fit-on-narrow-screen".to_string(),
            tasks: Vec::new(),
            title: String::new(),
            branch: String::new(),
            target: String::new(),
            warning: String::new(),
            archived: false,
        }],
        now: timestamp_for_date("2026-11-03"),
        ..Default::default()
    };

    let lines = sprint_tab::lines(&snapshot, 38);
    let text = render_lines_to_text(&lines, 38, 10);

    assert!(text.contains('…'));
}

#[test]
fn test_real_sprint_fixture_rendering() {
    let fixture_text = include_str!("fixtures/sprints/real-sprint.md");
    let sprint =
        tandem::sources::sprints::parse_sprint_md(fixture_text, "s-003-tandem-rust-cutover")
            .expect("real-sprint fixture should parse");
    let view = SprintView::from_sprint(sprint, &[]);

    let snapshot = StatusSnapshot {
        sprint: Some(view),
        phases: Vec::new(),
        now: timestamp_for_date("2026-11-17"),
        ..Default::default()
    };

    let lines = sprint_tab::lines(&snapshot, 100);
    let text = render_lines_to_text(&lines, 100, 15);

    assert!(text.contains("s-003-tandem-rust-cutover"));
    assert!(text.contains("overloaded"));
    assert!(text.contains("p-012-agent-workspace-package"));
    assert!(text.contains("p-013-agent-workspace-package"));
}
