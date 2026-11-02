mod common;

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use common::TempDir;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tandem::coordination::store::CoordStore;
use tandem::snapshot::StatusSnapshot;
use tandem::sources::sprints::{load_active_sprint, parse_sprint_md};
use tandem::status::app::StatusApp;
use tandem::status::render;

const REAL_SPRINT: &str = include_str!("fixtures/sprints/real-sprint.md");

fn sprint_md(
    status: &str,
    committed: Option<&str>,
    rows: &[(&str, &str, &str)],
    end: &str,
) -> String {
    let mut text = String::from("# Sprint s-001: Demo sprint\n\n## Metadata\n");
    text.push_str(&format!("- **Status**: {status}\n"));
    text.push_str("- **Start Date**: 2026-11-16\n");
    text.push_str(&format!("- **End Date**: {end}\n"));
    text.push_str("- **AI-Assisted Capacity Limit**: 5.0 Days\n");
    if let Some(c) = committed {
        text.push_str(&format!("- **Committed Capacity**: {c}\n"));
    }
    text.push_str("\n## Prioritized Scope (MoSCoW & RICE)\n\n");
    text.push_str("| Phase | Title | MoSCoW | Reach | Impact | Confidence | Effort | RICE Score | Spec Link |\n");
    text.push_str("|---|---|---|---|---|---|---|---|---|\n");
    for (phase, moscow, effort) in rows {
        text.push_str(&format!(
            "| {phase} | {phase} | {moscow} | 100 | 1 | 0.8 | {effort} | 10 | [x](../../specs/{phase}/impl.md) |\n"
        ));
    }
    text
}

fn write_sprint(ws: &Path, dir: &str, content: &str) {
    let folder = ws.join("sprints").join(dir);
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("sprint.md"), content).unwrap();
}

fn write_phase(ws: &Path, name: &str, tasks: &[(&str, bool)]) {
    let folder = ws.join("specs").join(name);
    fs::create_dir_all(&folder).unwrap();
    let mut impl_md = String::from("# impl\n\n## Tasks\n");
    for (id, done) in tasks {
        impl_md.push_str(&format!(
            "- [{}] {id}: task {id}\n",
            if *done { "x" } else { " " }
        ));
        fs::write(
            folder.join(format!("{id}.md")),
            format!(
                "## Status\n- [x] {}\n",
                if *done { "Done" } else { "Pending" }
            ),
        )
        .unwrap();
    }
    fs::write(folder.join("impl.md"), impl_md).unwrap();
}

fn snapshot_for(ws: &Path) -> StatusSnapshot {
    let store = CoordStore::new(ws.join("none"));
    tandem::snapshot::collect(ws, &store, std::time::Duration::from_secs(1))
}

fn at(date: &str) -> f64 {
    let day = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
    day.and_hms_opt(12, 0, 0).unwrap().and_utc().timestamp() as f64
}

#[test]
fn parses_the_real_sprint_file() {
    let sprint = parse_sprint_md(REAL_SPRINT, "s-003-tandem-rust-cutover").expect("parses");
    assert_eq!(sprint.id, "s-003-tandem-rust-cutover");
    assert_eq!(sprint.title, "tandem Rust cutover and release");
    assert_eq!(sprint.status, "planned");
    assert_eq!(sprint.start_date, "2026-11-16");
    assert_eq!(sprint.end_date, "2026-11-20");
    assert_eq!(sprint.capacity_days, 5.0);
    assert_eq!(
        sprint.committed_days, 7.5,
        "table rows are the single source"
    );
    assert_eq!(sprint.phases.len(), 2);
    assert_eq!(sprint.phases[0].name, "p-012-agent-workspace-package");
    assert_eq!(sprint.phases[0].moscow, "Must Have");
    assert_eq!(sprint.phases[1].effort, 3.0);
}

#[test]
fn committed_effort_is_not_double_counted() {
    let text = sprint_md(
        "open",
        Some("5.0 Days"),
        &[
            ("p-001-a", "Must Have", "2.0d"),
            ("p-002-b", "Should Have", "3.0d"),
        ],
        "2026-11-20",
    );
    assert_eq!(
        parse_sprint_md(&text, "s-001-demo").unwrap().committed_days,
        5.0
    );
    let text = sprint_md(
        "open",
        Some("9.9 Days"),
        &[
            ("p-001-a", "Must Have", "1.5d"),
            ("p-002-b", "Won't Have", "4d"),
        ],
        "2026-11-20",
    );
    assert_eq!(
        parse_sprint_md(&text, "s-001-demo").unwrap().committed_days,
        1.5
    );
    let text = sprint_md("open", Some("4.0 Days"), &[], "2026-11-20");
    assert_eq!(
        parse_sprint_md(&text, "s-001-demo").unwrap().committed_days,
        4.0
    );
}

#[test]
fn the_last_open_or_locked_sprint_is_the_active_one() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    write_sprint(
        ws,
        "s-001-first",
        &sprint_md("closed", None, &[], "2026-10-01"),
    );
    write_sprint(
        ws,
        "s-002-second",
        &sprint_md("open", None, &[], "2026-11-01"),
    );
    write_sprint(
        ws,
        "s-010-tenth",
        &sprint_md("locked", None, &[], "2026-12-01"),
    );
    write_sprint(
        ws,
        "s-011-next",
        &sprint_md("planned", None, &[], "2026-12-08"),
    );
    let active = load_active_sprint(ws).expect("an active sprint");
    assert_eq!(
        active.id, "s-010-tenth",
        "sorted directory order, last wins, planned never wins"
    );
    assert_eq!(active.status, "locked");
}

#[test]
fn no_active_sprint_means_none() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    assert!(load_active_sprint(ws).is_none(), "no sprints folder");
    write_sprint(
        ws,
        "s-001-first",
        &sprint_md("closed", None, &[], "2026-10-01"),
    );
    write_sprint(
        ws,
        "s-002-next",
        &sprint_md("planned", None, &[], "2026-10-08"),
    );
    assert!(load_active_sprint(ws).is_none());
}

#[test]
fn unrelated_directories_and_files_are_ignored() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    write_sprint(ws, "notes", &sprint_md("open", None, &[], "2026-11-20"));
    write_sprint(ws, "s-1-short", &sprint_md("open", None, &[], "2026-11-20"));
    fs::create_dir_all(ws.join("sprints/s-005-empty")).unwrap();
    fs::write(ws.join("sprints/README.md"), "# Sprints").unwrap();
    assert!(load_active_sprint(ws).is_none());
}

#[test]
fn malformed_sprint_files_never_panic() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    for (i, content) in ["", "# Sprint", "\u{0}\u{1}\u{2} binary \u{ff}", "| a | b |\n|---|---|\n| x |", "## Metadata\n- **AI-Assisted Capacity Limit**: lots\n- **Status**: open\n| p | t | Must Have | x | y | z | many | q | l |"].iter().enumerate() {
        write_sprint(ws, &format!("s-00{}-bad", i + 1), content);
    }
    let _ = load_active_sprint(ws);
    let mut snapshot = snapshot_for(ws);
    let _ = &mut snapshot;
    let mut app = StatusApp::new(
        ws.to_path_buf(),
        CoordStore::new(ws.join("none")),
        None,
        None,
    );
    let backend = TestBackend::new(100, 30);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
}

#[test]
fn a_huge_sprint_file_is_read_only_up_to_the_cap() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    let mut big = sprint_md(
        "open",
        None,
        &[("p-001-a", "Must Have", "1d")],
        "2026-11-20",
    );
    big.push_str(&"x".repeat(5_000_000));
    write_sprint(ws, "s-001-big", &big);
    let active = load_active_sprint(ws).expect("still parsed");
    assert_eq!(active.phases.len(), 1);
}

#[test]
fn snapshot_computes_progress_from_the_loaded_phases() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    write_phase(
        ws,
        "p-001-a",
        &[("t-001", true), ("t-002", true), ("t-003", false)],
    );
    write_phase(ws, "p-002-b", &[("t-001", false), ("t-002", false)]);
    write_sprint(
        ws,
        "s-001-demo",
        &sprint_md(
            "open",
            Some("5.0 Days"),
            &[
                ("p-001-a", "Must Have", "3d"),
                ("p-002-b", "Should Have", "2d"),
                ("p-003-missing", "Could Have", "1d"),
            ],
            "2026-11-20",
        ),
    );
    let view = snapshot_for(ws).sprint.expect("sprint view");
    assert_eq!((view.done, view.total), (2, 5));
    assert_eq!(view.must_remaining, 1);
    assert_eq!((view.phases[0].done, view.phases[0].total), (2, 3));
    assert_eq!(
        (view.phases[2].done, view.phases[2].total),
        (0, 0),
        "a phase without specs counts as empty"
    );
    assert_eq!(
        view.committed_days, 6.0,
        "rows are the source, not the metadata line"
    );
    assert!(view.overloaded, "6.0 committed against 5.0 capacity");
}

#[test]
fn overloaded_is_committed_above_capacity() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    write_sprint(
        ws,
        "s-001-demo",
        &sprint_md(
            "open",
            None,
            &[
                ("p-001-a", "Must Have", "4d"),
                ("p-002-b", "Should Have", "3d"),
            ],
            "2026-11-20",
        ),
    );
    let view = snapshot_for(ws).sprint.unwrap();
    assert!(view.overloaded);
    assert_eq!(view.committed_days, 7.0);
}

#[test]
fn days_left_uses_the_injected_clock() {
    assert_eq!(
        render::days_left("2026-11-20", at("2026-11-17")),
        "3 days left"
    );
    assert_eq!(
        render::days_left("2026-11-20", at("2026-11-19")),
        "1 day left"
    );
    assert_eq!(
        render::days_left("2026-11-20", at("2026-11-20")),
        "ends today"
    );
    assert_eq!(
        render::days_left("2026-11-20", at("2026-11-22")),
        "overdue 2d"
    );
    assert_eq!(render::days_left("", at("2026-11-22")), "");
    assert_eq!(render::days_left("someday", at("2026-11-22")), "");
}

fn snapshot_with_sprint(status: &str, rows: &[(&str, &str, &str)], now: f64) -> StatusSnapshot {
    let tmp = TempDir::new();
    let ws = tmp.path().to_path_buf();
    write_phase(&ws, "p-001-a", &[("t-001", true), ("t-002", false)]);
    write_sprint(
        &ws,
        "s-003-demo",
        &sprint_md(status, None, rows, "2026-11-20"),
    );
    let mut snapshot = snapshot_for(&ws);
    snapshot.now = now;
    snapshot
}

#[test]
fn sprint_line_shows_headline_progress_days_and_markers() {
    let snapshot = snapshot_with_sprint(
        "locked",
        &[
            ("p-001-a", "Must Have", "4d"),
            ("p-002-b", "Should Have", "3d"),
        ],
        at("2026-11-17"),
    );
    let line = render::sprint_line(&snapshot).expect("a headline");
    assert!(line.contains("s-003-demo"), "{line}");
    assert!(line.contains("Demo sprint"), "{line}");
    assert!(line.contains("1/2"), "{line}");
    assert!(line.contains("3 days left"), "{line}");
    assert!(line.contains("OVERLOADED"), "{line}");
    assert!(line.contains("locked"), "{line}");
}

#[test]
fn sprint_line_is_quiet_for_a_normal_open_sprint() {
    let snapshot =
        snapshot_with_sprint("open", &[("p-001-a", "Must Have", "2d")], at("2026-11-17"));
    let line = render::sprint_line(&snapshot).unwrap();
    assert!(
        !line.contains("OVERLOADED") && !line.contains("locked"),
        "{line}"
    );
}

#[test]
fn no_sprint_means_no_line_and_an_unchanged_summary() {
    let tmp = TempDir::new();
    let snapshot = snapshot_for(tmp.path());
    assert!(render::sprint_line(&snapshot).is_none());
    assert!(!render::summary_line(&snapshot).contains("s-0"));
}

#[test]
fn summary_line_gets_a_short_sprint_token() {
    let snapshot =
        snapshot_with_sprint("open", &[("p-001-a", "Must Have", "2d")], at("2026-11-17"));
    assert!(
        render::summary_line(&snapshot).starts_with("s-003 1/2 · "),
        "{}",
        render::summary_line(&snapshot)
    );
}

#[test]
fn sprint_text_lists_one_row_per_phase_with_dates_and_capacity() {
    let snapshot = snapshot_with_sprint(
        "open",
        &[
            ("p-001-a", "Must Have", "2d"),
            ("p-002-b", "Should Have", "1.5d"),
        ],
        at("2026-11-17"),
    );
    let text = render::sprint_text(&snapshot);
    assert!(
        text.contains("Must Have") && text.contains("p-001-a") && text.contains("1/2"),
        "{text}"
    );
    assert!(
        text.contains("Should Have") && text.contains("p-002-b") && text.contains("0/0"),
        "{text}"
    );
    assert!(
        text.contains("2026-11-16") && text.contains("2026-11-20"),
        "{text}"
    );
    assert!(text.contains("3.5") && text.contains("5.0"), "{text}");
}

fn screen(ws: &Path, width: u16, height: u16) -> String {
    let mut app = StatusApp::new(
        ws.to_path_buf(),
        CoordStore::new(ws.join("none")),
        None,
        None,
    );
    let backend = TestBackend::new(width, height);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
    let buf = term.backend().buffer().clone();
    let mut text = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            text.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
        }
        text.push('\n');
    }
    text
}

#[test]
fn status_tab_shows_the_sprint_only_when_one_is_active() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    write_phase(ws, "p-001-a", &[("t-001", true), ("t-002", false)]);
    assert!(
        !screen(ws, 120, 40).contains("s-003-demo"),
        "nothing without a sprint"
    );

    write_sprint(
        ws,
        "s-003-demo",
        &sprint_md(
            "open",
            None,
            &[("p-001-a", "Must Have", "2d")],
            "2026-11-20",
        ),
    );
    let shown = screen(ws, 120, 40);
    assert!(shown.contains("s-003-demo"), "headline: {shown}");
    let rows: Vec<&str> = shown.lines().collect();
    let headline = rows.iter().position(|r| r.contains("s-003-demo")).unwrap();
    let in_progress = rows.iter().position(|r| r.contains("In progress")).unwrap();
    assert!(headline < in_progress, "headline sits above In progress");

    write_sprint(
        ws,
        "s-003-demo",
        &sprint_md(
            "closed",
            None,
            &[("p-001-a", "Must Have", "2d")],
            "2026-11-20",
        ),
    );
    assert!(
        !screen(ws, 120, 40).contains("s-003-demo"),
        "a closed sprint disappears"
    );
}

#[test]
fn compact_mode_keeps_the_sprint_token() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    write_phase(ws, "p-001-a", &[("t-001", true), ("t-002", false)]);
    write_sprint(
        ws,
        "s-003-demo",
        &sprint_md(
            "open",
            None,
            &[("p-001-a", "Must Have", "2d")],
            "2026-11-20",
        ),
    );
    assert!(screen(ws, 120, 6).contains("s-003 1/2"));
}

fn python3() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn sprints_py(ws: &Path, args: &[&str]) -> Option<serde_json::Value> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("templates/skills/pm-new-sprint/scripts/sprints.py");
    let out = Command::new("python3")
        .arg(&script)
        .arg("--root")
        .arg(ws)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
        .output()
        .ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

#[test]
fn rust_and_sprints_py_agree_on_the_active_sprint_and_effort() {
    if !python3() {
        eprintln!("skipping: python3 not available");
        return;
    }
    let tmp = TempDir::new();
    let ws = tmp.path();
    write_sprint(
        ws,
        "s-001-first",
        &sprint_md(
            "closed",
            Some("5.0 Days"),
            &[("p-001-a", "Must Have", "5d")],
            "2026-10-01",
        ),
    );
    write_sprint(
        ws,
        "s-002-second",
        &sprint_md(
            "open",
            Some("5.0 Days"),
            &[
                ("p-001-a", "Must Have", "2d"),
                ("p-002-b", "Should Have", "3.5d"),
                ("p-003-c", "Won't Have", "9d"),
            ],
            "2026-11-20",
        ),
    );
    write_sprint(
        ws,
        "s-003-third",
        &sprint_md(
            "locked",
            Some("7.0 Days"),
            &[
                ("p-001-a", "Must Have", "4.5d"),
                ("p-002-b", "Should Have", "3d"),
            ],
            "2026-11-27",
        ),
    );
    write_sprint(
        ws,
        "s-004-later",
        &sprint_md("planned", None, &[], "2026-12-04"),
    );

    let resolved = sprints_py(ws, &["resolve"]).expect("resolve output");
    let rust = load_active_sprint(ws).expect("rust sprint");
    assert_eq!(resolved["sprint"], rust.id);

    let listed = sprints_py(ws, &["list"]).expect("list output");
    let by_id: HashMap<String, f64> = listed["sprints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["sprint"].as_str().unwrap().to_string(),
                s["committed_effort"].as_f64().unwrap(),
            )
        })
        .collect();
    assert_eq!(by_id[&rust.id], rust.committed_days);
    assert_eq!(by_id["s-002-second"], 5.5);
}
