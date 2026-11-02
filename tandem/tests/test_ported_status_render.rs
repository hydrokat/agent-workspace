mod common;

use std::collections::HashMap;
use tandem::coordination::schema::{Claim, Query, Session};
use tandem::snapshot::StatusSnapshot;
use tandem::sources::codebases::Codebase;
use tandem::sources::conflicts::Conflict;
use tandem::sources::gitinfo::{GitInfo, Worktree};
use tandem::sources::specs::{Phase, Task};
use tandem::status::render::{
    MIN_WIDTH, age, bar, conflicts_text, in_progress_text, phases_text, repo_lines, repos_text,
    session_row, summary_line,
};

const NOW: f64 = 10_000.0;

fn default_snap() -> StatusSnapshot {
    StatusSnapshot {
        phases: Vec::new(),
        entries: Vec::new(),
        sessions: Vec::new(),
        claims: Vec::new(),
        queries: Vec::new(),
        conflicts: Vec::new(),
        codebases: Vec::new(),
        workspace_git: None,
        codebase_git: HashMap::new(),
        coord_exists: true,
        now: NOW,
        sprint: None,
        ongoing_specs: Vec::new(),
        ongoing_tasks: Vec::new(),
    }
}

fn make_session(overrides: impl FnOnce(&mut Session)) -> Session {
    let mut s = Session {
        id: "implementer-1".to_string(),
        cli: "claude".to_string(),
        role: "implementer".to_string(),
        model: "sonnet".to_string(),
        pane: "%2".to_string(),
        cwd: "/w".to_string(),
        branch: "b".to_string(),
        worktree: "/w".to_string(),
        task: "p-001 t-003".to_string(),
        heartbeat: NOW - 12.0,
    };
    overrides(&mut s);
    s
}

fn default_session() -> Session {
    make_session(|_| {})
}

#[test]
fn test_summary_counts() {
    let mut s = default_snap();
    s.phases = vec![Phase::new(
        "p",
        vec![
            Task::new("t-1", "a", "Done"),
            Task::new("t-2", "b", "Pending"),
        ],
    )];
    s.sessions = vec![default_session()];
    s.claims = vec![Claim {
        phase: "p".to_string(),
        task: "t-2".to_string(),
        session: "implementer-1".to_string(),
        branch: String::new(),
        worktree: String::new(),
        paths: Vec::new(),
        heartbeat: NOW - 5.0,
    }];

    assert_eq!(
        summary_line(&s),
        "1/2 tasks · 1 agent · 1 claim · 0 open queries"
    );
}

#[test]
fn test_summary_warns_on_stale_and_conflicts() {
    let mut s = default_snap();
    s.claims = vec![Claim {
        phase: "p".to_string(),
        task: "t-2".to_string(),
        session: "gone".to_string(),
        branch: String::new(),
        worktree: String::new(),
        paths: Vec::new(),
        heartbeat: NOW - 5000.0,
    }];
    s.conflicts = vec![Conflict::new("t-1", "t-2", "x")];

    let line = summary_line(&s);
    assert!(line.contains("WARNING 1 stale"));
    assert!(line.contains("WARNING 1 conflicts"));
}

#[test]
fn test_open_queries_counted() {
    let mut s = default_snap();
    s.queries = vec![Query {
        id: "q-1".to_string(),
        status: "open".to_string(),
        asked_by: "s".to_string(),
        task: "t".to_string(),
        blocking_scope: "a".to_string(),
        created: 0.0,
        question: "?".to_string(),
        options: Vec::new(),
        answer: String::new(),
        answered_via: String::new(),
    }];

    assert!(summary_line(&s).contains("1 open query"));
}

#[test]
fn test_session_row_has_words_not_just_symbols() {
    let mut s = default_snap();
    s.sessions = vec![default_session()];

    let row = session_row(&s, 0);
    assert!(row.contains("heartbeat 12s ago"));
    assert!(row.contains("claude·sonnet"));
    assert!(row.contains("p-001 t-003"));
}

#[test]
fn test_session_row_stale() {
    let mut s = default_snap();
    s.sessions = vec![make_session(|sess| sess.heartbeat = NOW - 5000.0)];

    let row = session_row(&s, 0);
    assert!(row.contains("stale"));
}

#[test]
fn test_session_text_is_sanitized() {
    let mut s = default_snap();
    s.sessions = vec![make_session(|sess| {
        sess.task = "\x1b[31mred\x1b[0m".to_string();
    })];

    let row = session_row(&s, 0);
    assert!(!row.contains('\x1b'));
}

#[test]
fn test_stale_claim_names_gone_session() {
    let mut s = default_snap();
    s.claims = vec![Claim {
        phase: "p-001-x".to_string(),
        task: "t-002".to_string(),
        session: "3f2a".to_string(),
        branch: String::new(),
        worktree: String::new(),
        paths: Vec::new(),
        heartbeat: NOW - 840.0,
    }];

    let text = in_progress_text(&s);
    assert!(text.contains("WARNING stale"));
    assert!(text.contains("3f2a is gone"));
}

#[test]
fn test_empty_states() {
    let s = default_snap();
    assert!(in_progress_text(&s).contains("Nothing in progress"));
    assert!(phases_text(&s).contains("No specs yet"));
    assert!(repos_text(&s, 80).contains("No linked codebases"));
    assert!(repos_text(&s, 80).contains("workspace · not a git repository"));
    assert!(conflicts_text(&s).contains("No overlapping"));
}

#[test]
fn test_missing_coordination_dir_hint() {
    let mut s = default_snap();
    s.coord_exists = false;

    let text = in_progress_text(&s);
    assert!(text.contains("agent-coordination"));
}

#[test]
fn test_phase_warning_row_does_not_hide_others() {
    let mut s = default_snap();
    s.phases = vec![
        Phase::with_warning("bad", "bad/impl.md missing"),
        Phase::new("ok", vec![Task::new("t-1", "x", "Done")]),
    ];

    let text = phases_text(&s);
    assert!(text.contains("WARNING bad/impl.md missing"));
    assert!(text.contains("ok  "));
}

#[test]
fn test_bar_and_age() {
    assert_eq!(bar(1, 2, 4), "██░░");
    assert_eq!(bar(0, 0, 4), "░░░░");
    assert_eq!(age(12.0), "12s");
    assert_eq!(age(840.0), "14m");
    assert_eq!(age(7200.0), "2h");
}

#[test]
fn test_conflict_text() {
    let mut s = default_snap();
    s.conflicts = vec![Conflict::new("t-1", "t-2", "src")];

    assert_eq!(conflicts_text(&s), "WARNING t-1 and t-2 both touch src");
}

fn pkg_info() -> GitInfo {
    GitInfo {
        current: "trunk".to_string(),
        branches: vec![
            "trunk".to_string(),
            "feat/x".to_string(),
            "old-1".to_string(),
            "old-2".to_string(),
        ],
        worktrees: vec![
            Worktree {
                path: "/code/pkg".to_string(),
                branch: "trunk".to_string(),
                main: true,
            },
            Worktree {
                path: "/code/pkg-wt".to_string(),
                branch: "feat/x".to_string(),
                main: false,
            },
            Worktree {
                path: "/code/pkg-det".to_string(),
                branch: String::new(),
                main: false,
            },
        ],
    }
}

#[test]
fn test_repo_lines_show_branch_and_the_worktree_it_is_active_on() {
    let text = repo_lines("pkg", Some(&pkg_info()), 80).join("\n");
    assert!(text.contains("pkg · 4 branches · 2 worktrees"));
    assert!(text.contains("* trunk"));
    assert!(
        text.lines()
            .any(|l| l.contains("feat/x") && l.contains("→ ../pkg-wt"))
    );
    assert!(
        text.lines()
            .any(|l| l.contains("(detached)") && l.contains("→ ../pkg-det"))
    );
    assert!(text.lines().any(|l| l.trim() == "old-1"));
}

#[test]
fn test_repo_lines_main_checkout_branch_has_no_worktree_suffix() {
    let lines = repo_lines("pkg", Some(&pkg_info()), 80);
    let trunk_line = lines
        .iter()
        .find(|l| l.contains("trunk") && l.contains('*'))
        .unwrap();
    assert!(!trunk_line.contains('→'));
}

#[test]
fn test_repo_lines_cap_idle_branches() {
    let mut branches = vec!["m".to_string()];
    for i in 0..9 {
        branches.push(format!("b{i}"));
    }
    let info = GitInfo {
        current: "m".to_string(),
        branches,
        worktrees: vec![Worktree {
            path: "/r".to_string(),
            branch: "m".to_string(),
            main: true,
        }],
    };

    let text = repo_lines("r", Some(&info), 80).join("\n");
    assert!(text.contains("+4 more branches"));
    assert!(text.contains("b4"));
    assert!(!text.contains("b5"));
}

#[test]
fn test_repo_lines_fit_narrow_widths() {
    let info = GitInfo {
        current: "a-very-long-branch-name-here".to_string(),
        branches: vec![
            "a-very-long-branch-name-here".to_string(),
            "feat/other-long-one".to_string(),
        ],
        worktrees: vec![
            Worktree {
                path: "/r".to_string(),
                branch: "a-very-long-branch-name-here".to_string(),
                main: true,
            },
            Worktree {
                path: "/somewhere/else/very/deep/wt".to_string(),
                branch: "feat/other-long-one".to_string(),
                main: false,
            },
        ],
    };

    for width in [30, 40, 120] {
        let max_w = width.max(MIN_WIDTH);
        for line in repo_lines("r", Some(&info), width) {
            use unicode_width::UnicodeWidthStr;
            assert!(
                UnicodeWidthStr::width(line.as_str()) <= max_w,
                "width {max_w}: {line}"
            );
        }
    }
}

#[test]
fn test_repos_text_lists_workspace_then_each_codebase() {
    let temp = common::TempDir::new();
    let cbs = vec![
        Codebase {
            name: "pkg".to_string(),
            path: temp.path().to_path_buf(),
            exists: true,
            is_git: true,
        },
        Codebase {
            name: "plain".to_string(),
            path: temp.path().to_path_buf(),
            exists: true,
            is_git: false,
        },
        Codebase {
            name: "gone".to_string(),
            path: temp.path().join("x"),
            exists: false,
            is_git: false,
        },
    ];

    let mut s = default_snap();
    s.codebases = cbs;
    s.workspace_git = Some(GitInfo {
        current: "main".to_string(),
        branches: vec!["main".to_string()],
        worktrees: vec![Worktree {
            path: "/w".to_string(),
            branch: "main".to_string(),
            main: true,
        }],
    });
    s.codebase_git = HashMap::from([("pkg".to_string(), Some(pkg_info()))]);

    let text = repos_text(&s, 80);
    let ws_pos = text.find("workspace").unwrap();
    let pkg_pos = text.find("pkg ·").unwrap();
    let plain_pos = text.find("plain · not a git").unwrap();

    assert!(ws_pos < pkg_pos);
    assert!(pkg_pos < plain_pos);
    assert!(text.contains("gone · WARNING missing target"));
}

#[test]
fn test_repo_text_is_sanitized() {
    let info = GitInfo {
        current: "m".to_string(),
        branches: vec!["m".to_string(), "\x1b[31mevil".to_string()],
        worktrees: vec![Worktree {
            path: "/r".to_string(),
            branch: "m".to_string(),
            main: true,
        }],
    };

    let text = repo_lines("r", Some(&info), 80).join("\n");
    assert!(!text.contains('\x1b'));
}

#[test]
fn test_counts_are_singular_when_one() {
    let info = GitInfo {
        current: "m".to_string(),
        branches: vec!["m".to_string()],
        worktrees: vec![Worktree {
            path: "/r".to_string(),
            branch: "m".to_string(),
            main: true,
        }],
    };

    let lines = repo_lines("r", Some(&info), 80);
    assert_eq!(lines[0], "r · 1 branch · 0 worktrees");
}
