mod common;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use tandem::snapshot::StatusSnapshot;
use tandem::sources::ongoing::OngoingSpec;
use tandem::sources::specs::{Phase, Task};
use tandem::status::spec_tab;
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

#[test]
fn test_empty_state_when_no_ongoing_spec() {
    let snapshot = StatusSnapshot::default();
    let lines = spec_tab::lines(&snapshot, 80);
    assert_eq!(lines.len(), 1);
    let text = render_lines_to_text(&lines, 80, 5);
    assert!(text.contains("No ongoing spec."));
}

#[test]
fn test_one_ongoing_spec_with_tasks() {
    let phase = Phase {
        name: "p-014-agent-workspace-package".to_string(),
        title: "tandem Status window: Sprint, Spec and Task tabs".to_string(),
        branch: "feat/tandem-status-tabs".to_string(),
        target: "feat/tandem-agent-dx".to_string(),
        tasks: vec![
            Task::new("t-002", "Ongoing spec and task selectors", "Done"),
            Task::new("t-001", "Five-tab Status window", "Done"),
            Task::new("t-003", "Sprint tab: ongoing sprint in full", "In Progress"),
            Task::new("t-004", "Spec tab: ongoing spec and its tasks", "Pending"),
        ],
        warning: String::new(),
    };

    let snapshot = StatusSnapshot {
        ongoing_specs: vec![OngoingSpec {
            phase,
            next_up: false,
        }],
        ..Default::default()
    };

    let lines = spec_tab::lines(&snapshot, 100);
    let text = render_lines_to_text(&lines, 100, 15);

    assert!(text.contains("p-014-agent-workspace-package"));
    assert!(text.contains("tandem Status window"));
    assert!(text.contains("feat/tandem-status-tabs → feat/tandem-agent-dx"));
    assert!(text.contains("2 / 4 done"));

    let idx_t1 = text.find("t-001").expect("t-001 should be present");
    let idx_t2 = text.find("t-002").expect("t-002 should be present");
    let idx_t3 = text.find("t-003").expect("t-003 should be present");
    let idx_t4 = text.find("t-004").expect("t-004 should be present");

    assert!(idx_t1 < idx_t2, "tasks should be ordered by id");
    assert!(idx_t2 < idx_t3, "tasks should be ordered by id");
    assert!(idx_t3 < idx_t4, "tasks should be ordered by id");

    assert!(text.contains("▶"));
    assert!(text.contains("In Progress"));
    assert!(text.contains("Done"));
    assert!(text.contains("Pending"));
}

#[test]
fn test_next_up_spec_is_labelled() {
    let phase = Phase {
        name: "p-015-future-package".to_string(),
        title: "Future phase".to_string(),
        branch: String::new(),
        target: String::new(),
        tasks: vec![Task::new("t-001", "First task", "Pending")],
        warning: String::new(),
    };

    let snapshot = StatusSnapshot {
        ongoing_specs: vec![OngoingSpec {
            phase,
            next_up: true,
        }],
        ..Default::default()
    };

    let lines = spec_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);

    assert!(text.contains("next up: p-015-future-package"));
}

#[test]
fn test_several_ongoing_specs_stacked() {
    let p1 = Phase {
        name: "p-001-core".to_string(),
        title: "Core".to_string(),
        branch: "feat/core".to_string(),
        target: "main".to_string(),
        tasks: vec![Task::new("t-001", "Core task", "In Progress")],
        warning: String::new(),
    };
    let p2 = Phase {
        name: "p-002-docs".to_string(),
        title: "Docs".to_string(),
        branch: "feat/docs".to_string(),
        target: "main".to_string(),
        tasks: vec![Task::new("t-001", "Docs task", "In Progress")],
        warning: String::new(),
    };

    let snapshot = StatusSnapshot {
        ongoing_specs: vec![
            OngoingSpec {
                phase: p1,
                next_up: false,
            },
            OngoingSpec {
                phase: p2,
                next_up: false,
            },
        ],
        ..Default::default()
    };

    let lines = spec_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 15);

    assert!(text.contains("p-001-core"));
    assert!(text.contains("p-002-docs"));
}

#[test]
fn test_malformed_spec_warning_line() {
    let p_broken = Phase {
        name: "p-999-broken".to_string(),
        title: String::new(),
        branch: String::new(),
        target: String::new(),
        tasks: Vec::new(),
        warning: "impl.md missing".to_string(),
    };
    let p_ok = Phase {
        name: "p-001-ok".to_string(),
        title: "Valid phase".to_string(),
        branch: String::new(),
        target: String::new(),
        tasks: vec![Task::new("t-001", "Task", "Done")],
        warning: String::new(),
    };

    let snapshot = StatusSnapshot {
        ongoing_specs: vec![
            OngoingSpec {
                phase: p_broken,
                next_up: false,
            },
            OngoingSpec {
                phase: p_ok,
                next_up: false,
            },
        ],
        ..Default::default()
    };

    let lines = spec_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 15);

    assert!(text.contains("warning: impl.md missing"));
    assert!(text.contains("p-001-ok"));
}

#[test]
fn test_spec_with_zero_tasks() {
    let phase = Phase {
        name: "p-000-empty".to_string(),
        title: "Empty phase".to_string(),
        branch: String::new(),
        target: String::new(),
        tasks: Vec::new(),
        warning: String::new(),
    };

    let snapshot = StatusSnapshot {
        ongoing_specs: vec![OngoingSpec {
            phase,
            next_up: false,
        }],
        ..Default::default()
    };

    let lines = spec_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);

    assert!(text.contains("p-000-empty"));
    assert!(text.contains("0 / 0 done"));
}

#[test]
fn test_untrusted_text_sanitized() {
    let phase = Phase {
        name: "p-esc\x1b[31m".to_string(),
        title: "Title\x1b[0mInjected".to_string(),
        branch: "b\x1b[32mranch".to_string(),
        target: "t\x1b[33marget".to_string(),
        tasks: vec![Task::new("t-001", "Task\x1b[1mBold", "Done")],
        warning: String::new(),
    };

    let snapshot = StatusSnapshot {
        ongoing_specs: vec![OngoingSpec {
            phase,
            next_up: false,
        }],
        ..Default::default()
    };

    let lines = spec_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);

    assert!(!text.contains('\x1b'));
    assert!(text.contains("p-esc"));
    assert!(text.contains("TitleInjected"));
    assert!(text.contains("branch → target"));
    assert!(text.contains("TaskBold"));
}

#[test]
fn test_responsive_task_title_truncation() {
    let phase = Phase {
        name: "p-001".to_string(),
        title: "Title".to_string(),
        branch: String::new(),
        target: String::new(),
        tasks: vec![Task::new(
            "t-001",
            "This is an extraordinarily long task title that will definitely not fit on a 40 column screen",
            "Pending",
        )],
        warning: String::new(),
    };

    let snapshot = StatusSnapshot {
        ongoing_specs: vec![OngoingSpec {
            phase,
            next_up: false,
        }],
        ..Default::default()
    };

    let lines = spec_tab::lines(&snapshot, 40);
    let text = render_lines_to_text(&lines, 40, 10);

    assert!(text.contains('…'));
}
