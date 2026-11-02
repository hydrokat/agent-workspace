mod common;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tandem::coordination::schema::Claim;
use tandem::sanitize::clean;
use tandem::sources::codebases::list_codebases;
use tandem::sources::conflicts::find_conflicts;
use tandem::sources::gitinfo::git_info;
use tandem::sources::inprogress::parse_in_progress;
use tandem::sources::specs::load_phases;

const IMPL: &str = r#"# Implementation Plan: Thing

## Timeline
- **t-001**: First (Done)
- **t-002**: Second (In Progress)
- **t-003**: Third (Pending)

## Tasks
- [x] t-001: [New Feature] First
- [ ] t-002: [New Feature] Second
- [ ] t-003: [New Feature] Third
"#;

fn create_phase(root: &Path, name: &str, statuses: Option<HashMap<&str, &str>>) -> PathBuf {
    let d = root.join("specs").join(name);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("impl.md"), IMPL).unwrap();

    let default_statuses = HashMap::from([("t-001", "Done"), ("t-002", "In Progress")]);
    let statuses = statuses.unwrap_or(default_statuses);

    for (task, status) in statuses {
        let content = format!("# {task}: x\n\n## Status\n- [x] {status}\n");
        std::fs::write(d.join(format!("{task}.md")), content).unwrap();
    }
    d
}

#[test]
fn test_phase_progress_from_task_files() {
    let temp = common::TempDir::new();
    create_phase(temp.path(), "p-001-thing", None);

    let phases = load_phases(temp.path());
    assert_eq!(phases.len(), 1);
    let p = &phases[0];
    assert_eq!(p.name, "p-001-thing");
    assert_eq!((p.done(), p.total()), (1, 3));
    assert_eq!(
        p.tasks
            .iter()
            .map(|t| (t.id.as_str(), t.status.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("t-001", "Done"),
            ("t-002", "In Progress"),
            ("t-003", "Pending")
        ]
    );
}

#[test]
fn test_status_falls_back_to_timeline_then_checkbox() {
    let temp = common::TempDir::new();
    create_phase(temp.path(), "p-001-thing", Some(HashMap::new()));

    let phases = load_phases(temp.path());
    assert_eq!(phases.len(), 1);
    assert_eq!(
        phases[0]
            .tasks
            .iter()
            .map(|t| t.status.as_str())
            .collect::<Vec<_>>(),
        vec!["Done", "In Progress", "Pending"]
    );
}

#[test]
fn test_legacy_phase_layout() {
    let temp = common::TempDir::new();
    let d = temp.path().join("specs").join("phase-002");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("impl.md"), "- [x] task-001: a\n- [ ] task-002: b\n").unwrap();
    std::fs::write(d.join("task-001.md"), "## Status\n- [x] Done\n").unwrap();

    let phases = load_phases(temp.path());
    assert_eq!(phases.len(), 1);
    assert_eq!(phases[0].name, "phase-002");
    assert_eq!((phases[0].done(), phases[0].total()), (1, 2));
}

#[test]
fn test_no_specs_dir_is_empty() {
    let temp = common::TempDir::new();
    assert_eq!(load_phases(temp.path()), Vec::new());
}

#[test]
fn test_malformed_and_binary_files_do_not_raise() {
    let temp = common::TempDir::new();
    let d = create_phase(temp.path(), "p-001-thing", None);
    std::fs::write(d.join("t-002.md"), b"\xff\xfe\x00garbage").unwrap();
    std::fs::write(d.join("impl.md"), format!("{IMPL}{}", "\x1b[2J".repeat(5))).unwrap();

    let phases = load_phases(temp.path());
    assert_eq!(phases.len(), 1);
    assert_eq!(phases[0].total(), 3);
}

#[test]
fn test_phase_without_impl_is_reported_with_warning() {
    let temp = common::TempDir::new();
    std::fs::create_dir_all(temp.path().join("specs").join("p-009-empty")).unwrap();

    let phases = load_phases(temp.path());
    assert_eq!(phases.len(), 1);
    assert!(!phases[0].warning.is_empty());
    assert_eq!(phases[0].total(), 0);
}

#[test]
fn test_huge_impl_is_capped() {
    let temp = common::TempDir::new();
    let d = create_phase(temp.path(), "p-001-thing", None);
    std::fs::write(
        d.join("impl.md"),
        format!("{IMPL}{}", "x".repeat(5_000_000)),
    )
    .unwrap();

    let phases = load_phases(temp.path());
    assert_eq!(phases.len(), 1);
    assert_eq!(phases[0].total(), 3);
}

#[test]
fn test_escape_sequences_in_titles_are_stripped() {
    let temp = common::TempDir::new();
    let d = create_phase(temp.path(), "p-001-thing", None);
    std::fs::write(
        d.join("impl.md"),
        "- [ ] t-001: \x1b]0;pwned\x07\x1b[31mred\x1b[0m title\n",
    )
    .unwrap();

    let phases = load_phases(temp.path());
    assert_eq!(phases.len(), 1);
    assert!(!phases[0].tasks[0].title.contains('\x1b'));
    assert!(!phases[0].tasks[0].title.contains('\x07'));
}

#[test]
fn test_compiled_and_files_in_specs_ignored() {
    let temp = common::TempDir::new();
    create_phase(temp.path(), "p-001-thing", None);
    std::fs::create_dir_all(temp.path().join("specs").join("compiled")).unwrap();
    std::fs::write(temp.path().join("specs").join("in-progress.md"), "x").unwrap();

    let phases = load_phases(temp.path());
    assert_eq!(
        phases.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["p-001-thing"]
    );
}

const IN_PROGRESS_TEXT: &str = r#"# In Progress

## Example

### p-002-payments-api — t-003: Add refresh token rotation
- **Agent:** `@senior-backend-dev` (ID: `abc-1234`)

### p-001-thing — t-002: Real work
- **Agent:** `@senior-backend-dev` (ID: `sess-1`)
- **Location:** Branch `feat/x`
- **Ongoing:** Writing code.
- **Next step:** Test.
"#;

#[test]
fn test_in_progress_parses_entries_and_skips_template_examples() {
    let temp = common::TempDir::new();
    let file = temp.path().join("in-progress.md");
    std::fs::write(&file, IN_PROGRESS_TEXT).unwrap();

    let entries = parse_in_progress(&file);
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.spec, "p-001-thing");
    assert_eq!(e.title, "t-002: Real work");
    assert_eq!(e.fields.get("Ongoing"), Some(&"Writing code.".to_string()));
    assert!(e.fields.get("Agent").unwrap().contains("sess-1"));
}

#[test]
fn test_in_progress_missing_file() {
    let temp = common::TempDir::new();
    assert_eq!(parse_in_progress(&temp.path().join("nope.md")), Vec::new());
}

#[test]
fn test_sanitize_strips_csi_osc_and_controls() {
    assert_eq!(
        clean("a\x1b[31mb\x1b[0m\x1b]0;title\x07c\x00d\r\ne"),
        "abcd\ne"
    );
    assert_eq!(clean("[bold]x[/bold]"), "[bold]x[/bold]");
}

fn init_git_repo(path: &Path) {
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(["-c", "user.email=a@b.c", "-c", "user.name=t"])
            .args(args)
            .current_dir(path)
            .status()
            .expect("git execution");
        assert!(status.success());
    };
    std::fs::create_dir_all(path).unwrap();
    git(&["init", "-q", "-b", "trunk"]);
    std::fs::write(path.join("f"), "x").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "i"]);
    git(&["branch", "feat"]);
    let wt_path = path.parent().unwrap().join(format!(
        "{}-wt",
        path.file_name().unwrap().to_str().unwrap()
    ));
    git(&["worktree", "add", "-q", wt_path.to_str().unwrap(), "feat"]);
}

#[test]
fn test_git_info_reports_branches_and_worktrees() {
    let temp = common::TempDir::new();
    let r = temp.path().join("r");
    init_git_repo(&r);

    let info = git_info(&r, Duration::from_secs(5)).expect("git info");
    assert_eq!(info.current, "trunk");
    let branch_set: std::collections::HashSet<_> = info.branches.into_iter().collect();
    assert!(branch_set.contains("trunk"));
    assert!(branch_set.contains("feat"));
    assert_eq!(info.worktrees.len(), 2);

    let wt_summary: Vec<_> = info
        .worktrees
        .iter()
        .map(|w| {
            (
                Path::new(&w.path).file_name().unwrap().to_str().unwrap(),
                w.branch.as_str(),
                w.main,
            )
        })
        .collect();
    assert_eq!(
        wt_summary,
        vec![("r", "trunk", true), ("r-wt", "feat", false)]
    );
}

#[test]
fn test_git_info_keeps_slashes_in_branch_names() {
    let temp = common::TempDir::new();
    let r = temp.path().join("r");
    init_git_repo(&r);

    let status = std::process::Command::new("git")
        .args(["branch", "deep/name"])
        .current_dir(&r)
        .status()
        .unwrap();
    assert!(status.success());

    let wt2 = temp.path().join("wt2");
    let status = std::process::Command::new("git")
        .args(["worktree", "add", "-q", wt2.to_str().unwrap(), "deep/name"])
        .current_dir(&r)
        .status()
        .unwrap();
    assert!(status.success());

    let info = git_info(&r, Duration::from_secs(5)).expect("git info");
    let wt_branches: std::collections::HashSet<_> =
        info.worktrees.iter().map(|w| w.branch.as_str()).collect();
    assert!(wt_branches.contains("deep/name"));
}

#[test]
fn test_git_info_not_a_repo_is_none() {
    let temp = common::TempDir::new();
    assert!(git_info(temp.path(), Duration::from_secs(5)).is_none());
}

#[test]
fn test_git_info_missing_dir_is_none() {
    let temp = common::TempDir::new();
    assert!(git_info(&temp.path().join("nope"), Duration::from_secs(5)).is_none());
}

#[test]
#[cfg(unix)]
fn test_git_info_times_out() {
    use std::os::unix::fs::PermissionsExt;
    use tandem::sources::gitinfo::git_info_with_git;

    let temp = common::TempDir::new();
    let fake_git = temp.path().join("fake_git");
    std::fs::write(&fake_git, "#!/bin/sh\nsleep 5\n").unwrap();
    std::fs::set_permissions(&fake_git, std::fs::Permissions::from_mode(0o755)).unwrap();

    let result = git_info_with_git(
        temp.path(),
        Duration::from_millis(300),
        fake_git.to_str().unwrap(),
    );
    assert!(result.is_none());
}

#[test]
#[cfg(unix)]
fn test_codebases_from_symlinks_without_following_into_files() {
    use std::os::unix::fs::symlink;

    let temp = common::TempDir::new();
    let pkg = temp.path().join("pkg");
    init_git_repo(&pkg);

    let links = temp.path().join("ws").join("codebase-symlinks");
    std::fs::create_dir_all(&links).unwrap();

    symlink(&pkg, links.join("pkg")).unwrap();
    symlink(temp.path().join("gone"), links.join("dangling")).unwrap();

    let found = list_codebases(&temp.path().join("ws"));
    let map: HashMap<_, _> = found.into_iter().map(|c| (c.name.clone(), c)).collect();

    assert!(map["pkg"].exists);
    assert!(map["pkg"].is_git);
    assert!(!map["dangling"].exists);
}

#[test]
#[cfg(unix)]
fn test_codebases_skip_plain_files_like_the_codebase_map() {
    use std::os::unix::fs::symlink;

    let temp = common::TempDir::new();
    let pkg = temp.path().join("pkg");
    init_git_repo(&pkg);

    let links = temp.path().join("ws").join("codebase-symlinks");
    std::fs::create_dir_all(&links).unwrap();

    symlink(&pkg, links.join("pkg")).unwrap();
    std::fs::write(links.join("codebase-map.md"), "# map").unwrap();

    let found = list_codebases(&temp.path().join("ws"));
    assert_eq!(
        found.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["pkg"]
    );
}

#[test]
fn test_no_codebase_symlinks_dir() {
    let temp = common::TempDir::new();
    assert_eq!(list_codebases(temp.path()), Vec::new());
}

fn make_claim(task: &str, paths: &[&str]) -> Claim {
    Claim {
        phase: "p-001-x".to_string(),
        task: task.to_string(),
        session: "s".to_string(),
        branch: String::new(),
        worktree: String::new(),
        paths: paths.iter().map(|p| p.to_string()).collect(),
        heartbeat: 0.0,
    }
}

#[test]
fn test_conflicts_exact_and_prefix_overlap() {
    let claims = vec![
        make_claim("t-001", &["src/a.py", "docs"]),
        make_claim("t-002", &["docs/readme.md"]),
        make_claim("t-003", &["other/"]),
    ];
    let found = find_conflicts(&claims);
    assert_eq!(
        found
            .iter()
            .map(|c| (c.first.as_str(), c.second.as_str(), c.path.as_str()))
            .collect::<Vec<_>>(),
        vec![("t-001", "t-002", "docs")]
    );
}

#[test]
fn test_conflicts_none_when_disjoint_or_empty() {
    assert_eq!(
        find_conflicts(&[make_claim("t-001", &["a"]), make_claim("t-002", &["b"])]),
        Vec::new()
    );
    assert_eq!(
        find_conflicts(&[make_claim("t-001", &[]), make_claim("t-002", &[])]),
        Vec::new()
    );
}

#[test]
fn test_conflicts_ignore_traversal_and_normalize() {
    let claims = vec![
        make_claim("t-001", &["./src//a.py"]),
        make_claim("t-002", &["src/a.py"]),
    ];
    let found = find_conflicts(&claims);
    assert_eq!(found.len(), 1);
}
