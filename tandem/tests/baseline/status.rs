use std::fs;
use std::path::{Path, PathBuf};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use tandem::coordination::store::CoordStore;
use tandem::status::app::{StatusApp, StatusTab};

use crate::common::{TempDir, assert_golden};

fn buffer_text(buf: &Buffer) -> String {
    let mut lines = Vec::new();
    let area = buf.area;
    for y in area.y..area.y + area.height {
        let mut row = String::new();
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell((x, y)) {
                row.push_str(cell.symbol());
            }
        }
        if let Some(idx) = row.find("stale ") {
            let prefix = &row[..idx + 6];
            let rest = &row[idx + 6..];
            let normalized = if let Some(ago_idx) = rest.find(" ago") {
                format!("{prefix}[AGE]{}", &rest[ago_idx + 4..])
            } else {
                let after = rest.trim_start_matches(|c: char| {
                    c.is_ascii_digit() || c == 'h' || c == 'm' || c == 's'
                });
                format!("{prefix}[AGE]{after}")
            };
            lines.push(normalized);
        } else {
            lines.push(row);
        }
    }
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

fn setup_test_workspace(root: &Path) -> (PathBuf, PathBuf) {
    let ws = root.join("ws");
    let phase = ws.join("specs").join("p-001-baseline");
    fs::create_dir_all(&phase).unwrap();
    fs::write(
        phase.join("impl.md"),
        "# Baseline Implementation Plan\n\n## Tasks\n- [x] t-001: Task One\n- [ ] t-002: Task Two\n",
    )
    .unwrap();
    fs::write(
        phase.join("t-001.md"),
        "# t-001: Task One\n\n## Status\n- [x] Done\n",
    )
    .unwrap();
    fs::write(
        phase.join("t-002.md"),
        "# t-002: Task Two\n\n## Status\n- [ ] Pending\n",
    )
    .unwrap();
    fs::write(
        ws.join("specs").join("in-progress.md"),
        "# In Progress\n\n### p-001-baseline — Task 002: Task Two\n- **Agent:** @implementer-1\n- **Ongoing:** Implementing\n- **Next step:** Testing\n",
    )
    .unwrap();

    let sprint_dir = ws.join("specs").join("s-001-baseline");
    fs::create_dir_all(&sprint_dir).unwrap();
    fs::write(
        sprint_dir.join("sprint.md"),
        "# Sprint 001\n\nStatus: active\n\n## Deliverables\n- [ ] Goal 1\n",
    )
    .unwrap();

    let coord = root.join("coord");
    let sess_dir = coord.join("sessions");
    fs::create_dir_all(&sess_dir).unwrap();
    let sess_json = serde_json::json!({
        "id": "impl-1",
        "cli": "claude",
        "role": "implementer",
        "model": "sonnet",
        "pane": "%1",
        "cwd": ws.to_string_lossy(),
        "branch": "feat/baseline",
        "worktree": ws.to_string_lossy(),
        "task": "p-001 t-002",
        "heartbeat": 1_700_000_000.0,
    });
    fs::write(sess_dir.join("impl-1.json"), sess_json.to_string()).unwrap();

    let query_dir = coord.join("queries");
    fs::create_dir_all(&query_dir).unwrap();
    let query_json = serde_json::json!({
        "id": "q-1",
        "asker": "impl-1",
        "question": "Baseline question?",
        "blocking_scope": "task",
        "timestamp": 1_700_000_000.0,
    });
    fs::write(query_dir.join("q-1.json"), query_json.to_string()).unwrap();

    (ws, coord)
}

#[test]
fn test_golden_status_window_renders() {
    let tmp = TempDir::new();
    let (ws, coord) = setup_test_workspace(tmp.path());

    let tabs = [
        (StatusTab::Status, "status"),
        (StatusTab::Sprint, "sprint"),
        (StatusTab::Spec, "spec"),
        (StatusTab::Task, "task"),
        (StatusTab::Queries, "queries"),
    ];
    let widths = [70, 100, 150];

    for (tab, tab_name) in tabs {
        for width in widths {
            let mut app = StatusApp::new(ws.clone(), CoordStore::new(coord.clone()), None, None);
            app.set_tab(tab);

            let backend = TestBackend::new(width, 20);
            let mut term = Terminal::new(backend).unwrap();
            term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
            let text = buffer_text(term.backend().buffer());

            let golden_name = format!("status/{tab_name}_{width}.txt");
            assert_golden(&golden_name, &text);
        }
    }
}
