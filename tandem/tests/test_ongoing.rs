mod common;

use common::TempDir;
use std::fs;
use std::time::Duration;
use tandem::coordination::schema::{Claim, Session};
use tandem::coordination::store::CoordStore;
use tandem::snapshot::collect;
use tandem::sources::inprogress::InProgressEntry;
use tandem::sources::ongoing::{ongoing_specs, ongoing_tasks};
use tandem::sources::specs::{Phase, Task, load_phases};
use tandem::sources::sprints::{Sprint, SprintPhase};

fn make_phase_with_tasks(name: &str, tasks: &[(&str, &str)]) -> Phase {
    let task_objs = tasks
        .iter()
        .map(|(id, status)| Task {
            id: id.to_string(),
            title: format!("Task {id}"),
            status: status.to_string(),
            objectives_done: 1,
            objectives_total: 2,
        })
        .collect();
    Phase {
        name: name.to_string(),
        title: format!("Phase {name}"),
        branch: "feat/branch".to_string(),
        target: "trunk".to_string(),
        tasks: task_objs,
        warning: String::new(),
        archived: false,
    }
}

fn make_claim(phase: &str, task: &str, session: &str, heartbeat: f64) -> Claim {
    Claim {
        phase: phase.to_string(),
        task: task.to_string(),
        session: session.to_string(),
        branch: "feat/task".to_string(),
        worktree: "/wt".to_string(),
        paths: vec!["src/lib.rs".to_string()],
        heartbeat,
    }
}

fn make_sprint(phases: &[(&str, &str, f64)]) -> Sprint {
    Sprint {
        id: "s-001".to_string(),
        title: "Sprint 1".to_string(),
        status: "open".to_string(),
        start_date: "2026-11-01".to_string(),
        end_date: "2026-11-07".to_string(),
        capacity_days: 5.0,
        committed_days: phases.iter().map(|p| p.2).sum(),
        phases: phases
            .iter()
            .map(|(name, moscow, effort)| SprintPhase {
                name: name.to_string(),
                moscow: moscow.to_string(),
                effort: *effort,
            })
            .collect(),
    }
}

#[test]
fn test_ongoing_spec_in_progress_task() {
    let p1 = make_phase_with_tasks(
        "p-001-core",
        &[("t-001", "In Progress"), ("t-002", "Pending")],
    );
    let p2 = make_phase_with_tasks("p-002-docs", &[("t-001", "Pending")]);
    let phases = vec![p1, p2];

    let ongoing = ongoing_specs(&phases, &[], &[], None);
    assert_eq!(ongoing.len(), 1);
    assert_eq!(ongoing[0].phase.name, "p-001-core");
    assert!(!ongoing[0].next_up);
}

#[test]
fn test_ongoing_spec_in_review_only() {
    let p1 = make_phase_with_tasks("p-001-core", &[("t-001", "In Review"), ("t-002", "Done")]);
    let phases = vec![p1];

    let ongoing = ongoing_specs(&phases, &[], &[], None);
    assert_eq!(ongoing.len(), 1);
    assert_eq!(ongoing[0].phase.name, "p-001-core");
    assert!(!ongoing[0].next_up);
}

#[test]
fn test_ongoing_spec_claim_only() {
    let p1 = make_phase_with_tasks("p-001-core", &[("t-001", "Pending")]);
    let phases = vec![p1];
    let claims = vec![make_claim("p-001-core", "t-001", "sess-1", 1_700_000_000.0)];

    let ongoing = ongoing_specs(&phases, &claims, &[], None);
    assert_eq!(ongoing.len(), 1);
    assert_eq!(ongoing[0].phase.name, "p-001-core");
    assert!(!ongoing[0].next_up);
}

#[test]
fn test_ongoing_spec_inprogress_md_only() {
    let p1 = make_phase_with_tasks("p-001-core", &[("t-001", "Pending")]);
    let phases = vec![p1];
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("Ongoing".to_string(), "t-001".to_string());
    let entries = vec![InProgressEntry {
        spec: "p-001-core".to_string(),
        title: "Core work".to_string(),
        fields,
    }];

    let ongoing = ongoing_specs(&phases, &[], &entries, None);
    assert_eq!(ongoing.len(), 1);
    assert_eq!(ongoing[0].phase.name, "p-001-core");
    assert!(!ongoing[0].next_up);
}

#[test]
fn test_ongoing_spec_next_up_active_sprint_moscow_order() {
    let p1 = make_phase_with_tasks("p-001-should", &[("t-001", "Pending")]);
    let p2 = make_phase_with_tasks("p-002-must", &[("t-001", "Pending")]);
    let p3 = make_phase_with_tasks("p-003-could", &[("t-001", "Pending")]);
    let phases = vec![p1, p2, p3];

    let sprint = make_sprint(&[
        ("p-001-should", "Should Have", 1.0),
        ("p-002-must", "Must Have", 2.0),
        ("p-003-could", "Could Have", 1.0),
    ]);

    let ongoing = ongoing_specs(&phases, &[], &[], Some(&sprint));
    assert_eq!(ongoing.len(), 1);
    assert_eq!(ongoing[0].phase.name, "p-002-must");
    assert!(ongoing[0].next_up);
}

#[test]
fn test_ongoing_spec_no_sprint_empty() {
    let p1 = make_phase_with_tasks("p-001-core", &[("t-001", "Pending")]);
    let phases = vec![p1];

    let ongoing = ongoing_specs(&phases, &[], &[], None);
    assert!(ongoing.is_empty());
}

#[test]
fn test_ongoing_spec_several_ongoing() {
    let p1 = make_phase_with_tasks("p-001-core", &[("t-001", "In Progress")]);
    let p2 = make_phase_with_tasks("p-002-api", &[("t-001", "In Review")]);
    let phases = vec![p1, p2];

    let ongoing = ongoing_specs(&phases, &[], &[], None);
    assert_eq!(ongoing.len(), 2);
    assert_eq!(ongoing[0].phase.name, "p-001-core");
    assert_eq!(ongoing[1].phase.name, "p-002-api");
    assert!(!ongoing[0].next_up);
    assert!(!ongoing[1].next_up);
}

#[test]
fn test_ongoing_task_join_with_claim_and_entry() {
    let p1 = make_phase_with_tasks("p-001-core", &[("t-001", "In Progress")]);
    let phases = vec![p1];
    let claims = vec![make_claim("p-001-core", "t-001", "s-123", 1_700_000_000.0)];
    let sessions = vec![Session {
        id: "s-123".to_string(),
        name: String::new(),
        cli: "claude".to_string(),
        role: "implementer".to_string(),
        model: "sonnet".to_string(),
        pane: "%5".to_string(),
        cwd: "/w".to_string(),
        branch: "feat".to_string(),
        worktree: "/wt".to_string(),
        task: "t-001".to_string(),
        heartbeat: 1_700_000_000.0,
    }];
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("Next step".to_string(), "Add unit tests".to_string());
    let entries = vec![InProgressEntry {
        spec: "p-001-core".to_string(),
        title: "Core".to_string(),
        fields,
    }];

    let tasks = ongoing_tasks(&phases, &claims, &sessions, &entries, 1_700_000_050.0);
    assert_eq!(tasks.len(), 1);
    let t = &tasks[0];
    assert_eq!(t.phase, "p-001-core");
    assert_eq!(t.task_id, "t-001");
    assert_eq!(t.status, "In Progress");
    assert_eq!(t.session_pane.as_deref(), Some("%5"));
    assert!(!t.stale);
    assert!(!t.status_mismatch);
    assert_eq!(t.next_step.as_deref(), Some("Add unit tests"));
}

#[test]
fn test_ongoing_task_stale_claim() {
    let p1 = make_phase_with_tasks("p-001-core", &[("t-001", "In Progress")]);
    let phases = vec![p1];
    let claims = vec![make_claim("p-001-core", "t-001", "s-123", 1_700_000_000.0)];

    let tasks = ongoing_tasks(&phases, &claims, &[], &[], 1_700_000_700.0);
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].stale);
    assert!(!tasks[0].status_mismatch);
}

#[test]
fn test_ongoing_task_claim_on_not_in_progress_is_mismatch() {
    let p1 = make_phase_with_tasks("p-001-core", &[("t-001", "Done")]);
    let phases = vec![p1];
    let claims = vec![make_claim("p-001-core", "t-001", "s-123", 1_700_000_000.0)];

    let tasks = ongoing_tasks(&phases, &claims, &[], &[], 1_700_000_050.0);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].status, "Done");
    assert!(tasks[0].status_mismatch);
}

#[test]
fn test_ongoing_task_unclaimed_in_progress() {
    let p1 = make_phase_with_tasks("p-001-core", &[("t-001", "In Progress")]);
    let phases = vec![p1];

    let tasks = ongoing_tasks(&phases, &[], &[], &[], 1_700_000_050.0);
    assert_eq!(tasks.len(), 1);
    assert!(tasks[0].claim.is_none());
    assert!(!tasks[0].stale);
    assert!(!tasks[0].status_mismatch);
}

#[test]
fn test_specs_phase_title_branch_target_and_task_objectives() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    let phase_dir = ws.join("specs").join("p-014-status-tabs");
    fs::create_dir_all(&phase_dir).unwrap();

    let impl_md = r#"# Implementation Plan: Status Window Five Tabs

## Branch Strategy
- **Working Branch**: feat/tandem-tabs
- **Merge Target**: trunk

## Tasks
- [ ] t-001: Model
- [ ] t-002: Selectors
"#;
    fs::write(phase_dir.join("impl.md"), impl_md).unwrap();

    let t1_md = r#"# t-001: Model

## Objectives
- [x] First item
- [X] Second item
- [ ] Third item

## Testing
- [x] Test item not in objectives
"#;
    fs::write(phase_dir.join("t-001.md"), t1_md).unwrap();

    let phases = load_phases(ws);
    assert_eq!(phases.len(), 1);
    let p = &phases[0];
    assert_eq!(p.title, "Status Window Five Tabs");
    assert_eq!(p.branch, "feat/tandem-tabs");
    assert_eq!(p.target, "trunk");
    assert_eq!(p.tasks.len(), 2);
    assert_eq!(p.tasks[0].objectives_done, 2);
    assert_eq!(p.tasks[0].objectives_total, 3);
}

#[test]
fn test_old_snapshot_json_deserializes_without_new_fields() {
    let json = r#"{
        "phases": [],
        "entries": [],
        "sessions": [],
        "claims": [],
        "queries": [],
        "conflicts": [],
        "codebases": [],
        "coord_exists": true,
        "now": 1700000000.0
    }"#;
    let snap: tandem::snapshot::StatusSnapshot = serde_json::from_str(json).unwrap();
    assert!(snap.ongoing_specs.is_empty());
    assert!(snap.ongoing_tasks.is_empty());
}

#[test]
fn test_snapshot_collect_fills_ongoing_specs_and_tasks() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    let phase_dir = ws.join("specs").join("p-001-core");
    fs::create_dir_all(&phase_dir).unwrap();
    fs::write(
        phase_dir.join("impl.md"),
        "# Implementation Plan: Core\n\n- [ ] t-001: Task 1\n",
    )
    .unwrap();
    fs::write(
        phase_dir.join("t-001.md"),
        "## Status\n- [x] In Progress\n\n## Objectives\n- [ ] Obj 1\n",
    )
    .unwrap();

    let coord = ws.join("coord");
    fs::create_dir_all(&coord).unwrap();
    let store = CoordStore::new(coord);

    let snap = collect(ws, &store, Duration::from_millis(50));
    assert_eq!(snap.ongoing_specs.len(), 1);
    assert_eq!(snap.ongoing_specs[0].phase.name, "p-001-core");
    assert_eq!(snap.ongoing_tasks.len(), 1);
    assert_eq!(snap.ongoing_tasks[0].task_id, "t-001");
}
