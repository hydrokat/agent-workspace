mod common;

use std::fs;
use std::path::Path;

use common::TempDir;
use tandem::snapshot::{SprintPhaseView, StatusSnapshot};
use tandem::sources::specs::{Phase, load_phases};
use tandem::status::render::sprint_text;

fn write_sprint(ws: &Path, dir: &str, content: &str) {
    let folder = ws.join("sprints").join(dir);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("sprint.md"), content).unwrap();
}

fn collect_snapshot(ws: &Path) -> StatusSnapshot {
    let store = tandem::coordination::store::CoordStore::new(ws.to_path_buf());
    tandem::snapshot::collect(ws, &store, std::time::Duration::from_secs(1))
}

fn sprint_md(phase: &str) -> String {
    format!(
        r#"# Sprint s-001: Demo sprint

## Metadata
- **Status**: open
- **Start Date**: 2026-11-01
- **End Date**: 2026-11-08
- **AI-Assisted Capacity Limit**: 5.0 Days

## Prioritized Scope (MoSCoW & RICE)

| Phase | Title | MoSCoW | Reach | Impact | Confidence | Effort | RICE Score | Spec Link |
|---|---|---|---|---|---|---|---|---|
| {phase} | {phase} | Must Have | 100 | 1 | 0.8 | 2.5d | 10 | [x](../../specs/compiled/completed.md) |
"#
    )
}

#[test]
fn test_archived_phase_reports_done_equals_total_and_renders_archived_mark() {
    let temp = TempDir::new();
    let ws = temp.path();

    let compiled_dir = ws.join("specs").join("compiled");
    fs::create_dir_all(&compiled_dir).unwrap();
    let completed_md = include_str!("fixtures/archive/specs/compiled/completed.md");
    fs::write(compiled_dir.join("completed.md"), completed_md).unwrap();

    write_sprint(ws, "s-001-demo", &sprint_md("p-010-archived"));

    let snapshot = collect_snapshot(ws);
    let sprint = snapshot.sprint.as_ref().expect("active sprint");
    assert_eq!(sprint.total, 2);
    assert_eq!(sprint.done, 2);
    assert_eq!(sprint.phases.len(), 1);
    assert!(sprint.phases[0].archived);
    assert_eq!(sprint.phases[0].done, 2);
    assert_eq!(sprint.phases[0].total, 2);

    let rendered = sprint_text(&snapshot);
    assert!(rendered.contains("(archived)"));
    assert!(rendered.contains("2/2"));
}

#[test]
fn test_disk_wins_over_archive() {
    let temp = TempDir::new();
    let ws = temp.path();

    let compiled_dir = ws.join("specs").join("compiled");
    fs::create_dir_all(&compiled_dir).unwrap();
    let completed_md = include_str!("fixtures/archive/specs/compiled/completed.md");
    fs::write(compiled_dir.join("completed.md"), completed_md).unwrap();

    let disk_phase = ws.join("specs").join("p-010-archived");
    fs::create_dir_all(&disk_phase).unwrap();
    fs::write(
        disk_phase.join("impl.md"),
        "# Implementation Plan: Disk Title\n\n## Tasks\n- [x] t-001: Task 1\n- [ ] t-002: Task 2\n- [ ] t-003: Task 3\n",
    )
    .unwrap();
    fs::write(disk_phase.join("t-001.md"), "- [x] Done\n").unwrap();
    fs::write(disk_phase.join("t-002.md"), "- [ ] In Progress\n").unwrap();
    fs::write(disk_phase.join("t-003.md"), "- [ ] Pending\n").unwrap();

    write_sprint(ws, "s-001-demo", &sprint_md("p-010-archived"));

    let snapshot = collect_snapshot(ws);
    let sprint = snapshot.sprint.as_ref().expect("active sprint");
    assert_eq!(sprint.total, 3);
    assert_eq!(sprint.done, 1);
    assert!(!sprint.phases[0].archived);

    let phases = load_phases(ws);
    let p = phases.iter().find(|p| p.name == "p-010-archived").unwrap();
    assert_eq!(p.title, "Disk Title");
    assert!(!p.archived);
}

#[test]
fn test_missing_oversized_malformed_archive_does_not_panic() {
    let temp = TempDir::new();
    let ws = temp.path();
    let compiled_dir = ws.join("specs").join("compiled");
    fs::create_dir_all(&compiled_dir).unwrap();

    let phases1 = load_phases(ws);
    assert!(phases1.is_empty());

    fs::write(compiled_dir.join("completed.md"), b"\x00\xff\xfe\xca\xfe").unwrap();
    let phases2 = load_phases(ws);
    assert!(phases2.is_empty());

    let oversized = "## p-001-big: Big\n\n| Task | Title |\n|---|---|\n| t-001 | A |\n".repeat(50_000);
    fs::write(compiled_dir.join("completed.md"), oversized).unwrap();
    let phases3 = load_phases(ws);
    assert!(!phases3.is_empty());
}

#[test]
fn test_escape_sequences_in_archive_cleaned() {
    let temp = TempDir::new();
    let ws = temp.path();
    let compiled_dir = ws.join("specs").join("compiled");
    fs::create_dir_all(&compiled_dir).unwrap();

    let text = "## p-002-esc: \x1b[31mRed Title\x1b[0m\n\n### Tasks\n\n| Task | Title | Done |\n|---|---|---|\n| t-001 | \x1b[32mGreen\x1b[0m Task | ✅ |\n";
    fs::write(compiled_dir.join("completed.md"), text).unwrap();

    let phases = load_phases(ws);
    let p = phases.iter().find(|p| p.name == "p-002-esc").unwrap();
    assert_eq!(p.title, "Red Title");
    assert_eq!(p.tasks[0].title, "Green Task");
    assert!(p.archived);
}

#[test]
fn test_old_serialized_snapshot_without_archived_field_loads() {
    let phase_json = r#"{"name":"p-001-old","title":"Old","branch":"","target":"","tasks":[],"warning":""}"#;
    let phase: Phase = serde_json::from_str(phase_json).unwrap();
    assert!(!phase.archived);

    let sprint_phase_json = r#"{"name":"p-001-old","moscow":"Must Have","effort":2.0,"done":1,"total":1}"#;
    let sprint_phase: SprintPhaseView = serde_json::from_str(sprint_phase_json).unwrap();
    assert!(!sprint_phase.archived);
}
