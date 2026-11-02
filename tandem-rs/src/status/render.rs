use crate::sanitize::clean;
use crate::snapshot::StatusSnapshot;
use crate::sources::gitinfo::GitInfo;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const MAX_IDLE_BRANCHES: usize = 5;
pub const MIN_WIDTH: usize = 24;

pub fn age(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s < 90 {
        format!("{s}s")
    } else if s < 5400 {
        format!("{}m", s / 60)
    } else {
        format!("{}h", s / 3600)
    }
}

pub fn bar(done: usize, total: usize, width: usize) -> String {
    let filled = if total > 0 {
        ((width as f64 * done as f64 / total as f64).round() as usize).min(width)
    } else {
        0
    };
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn summary_line(s: &StatusSnapshot) -> String {
    let done: usize = s.phases.iter().map(|p| p.done()).sum();
    let total: usize = s.phases.iter().map(|p| p.total()).sum();
    let mut parts = vec![
        format!("{done}/{total} tasks"),
        count(s.sessions.len(), "agent", "agents"),
        count(s.claims.len(), "claim", "claims"),
    ];

    let stale = s.stale_claims();
    if !stale.is_empty() {
        parts.push(format!("WARNING {} stale", stale.len()));
    }

    if !s.conflicts.is_empty() {
        parts.push(format!("WARNING {} conflicts", s.conflicts.len()));
    }

    let open = s.open_queries();
    parts.push(format!(
        "{open} open {}",
        if open == 1 { "query" } else { "queries" }
    ));

    parts.join(" · ")
}

pub fn session_row(s: &StatusSnapshot, index: usize) -> String {
    let sess = &s.sessions[index];
    let state = if s.stale(sess.heartbeat) {
        "stale"
    } else {
        "heartbeat"
    };

    let where_clause = if !sess.branch.is_empty() {
        format!("  branch {}", clean(&sess.branch))
    } else {
        String::new()
    };

    let raw_task = clean(&sess.task);
    let task = if raw_task.is_empty() {
        "idle"
    } else {
        &raw_task
    };

    let cleaned_model = clean(&sess.model);
    let model = if cleaned_model.is_empty() {
        "?"
    } else {
        &cleaned_model
    };

    format!(
        "{}  {}·{}  {}{where_clause}  {state} {} ago",
        clean(&sess.id),
        clean(&sess.cli),
        model,
        task,
        age(s.now - sess.heartbeat)
    )
}

pub fn in_progress_text(s: &StatusSnapshot) -> String {
    let mut lines = Vec::new();
    let session_ids: HashSet<&str> = s.sessions.iter().map(|x| x.id.as_str()).collect();

    for claim in &s.claims {
        let flag = if s.stale(claim.heartbeat) {
            let gone = if session_ids.contains(claim.session.as_str()) {
                ""
            } else {
                " (session "
            };
            if gone.is_empty() {
                format!(
                    "  WARNING stale: last heartbeat {} ago",
                    age(s.now - claim.heartbeat)
                )
            } else {
                format!(
                    "  WARNING stale: last heartbeat {} ago (session {} is gone)",
                    age(s.now - claim.heartbeat),
                    clean(&claim.session)
                )
            }
        } else {
            String::new()
        };

        lines.push(format!(
            "claim {} {} by {}{flag}",
            clean(&claim.phase),
            clean(&claim.task),
            clean(&claim.session)
        ));
    }

    for e in &s.entries {
        lines.push(format!("in-progress.md: {} — {}", e.spec, e.title));
        for key in &["Ongoing", "Next step"] {
            if let Some(val) = e.fields.get(*key) {
                lines.push(format!("    {key}: {val}"));
            }
        }
    }

    if lines.is_empty() {
        lines.push("Nothing in progress.".to_string());
    }

    if !s.coord_exists {
        lines.push("Coordination folder not found: sessions unknown. Agents register via the agent-coordination skill.".to_string());
    }

    lines.join("\n")
}

pub fn phases_text(s: &StatusSnapshot) -> String {
    if s.phases.is_empty() {
        return "No specs yet. A planner can create one.".to_string();
    }

    let mut lines = Vec::new();
    for p in &s.phases {
        if !p.warning.is_empty() {
            lines.push(format!("WARNING {}", p.warning));
            continue;
        }

        lines.push(format!(
            "{}  {}  {}/{}",
            p.name,
            bar(p.done(), p.total(), 12),
            p.done(),
            p.total()
        ));

        for t in &p.tasks {
            lines.push(format!("    {}  {:<11} {}", t.id, t.status, t.title));
        }
    }

    lines.join("\n")
}

pub fn fit(text: &str, width: usize) -> String {
    let text_width = UnicodeWidthStr::width(text);
    if width <= 1 || text_width <= width {
        return text.to_string();
    }
    let target_width = width.saturating_sub(1);
    let mut current_width = 0;
    let mut end_idx = 0;
    for (idx, ch) in text.char_indices() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > target_width {
            break;
        }
        current_width += ch_width;
        end_idx = idx + ch.len_utf8();
    }
    format!("{}…", &text[..end_idx])
}

fn worktree_label(path: &str, main_path: &str) -> String {
    if main_path.is_empty() {
        return clean(path);
    }
    let rel = relative_path(Path::new(path), Path::new(main_path));
    let rel_str = rel
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    clean(if rel_str.len() < path.len() {
        &rel_str
    } else {
        path
    })
}

fn relative_path(path: &Path, base: &Path) -> Option<PathBuf> {
    let mut path_comps = path.components().peekable();
    let mut base_comps = base.components().peekable();

    while let (Some(p), Some(b)) = (path_comps.peek(), base_comps.peek()) {
        if p == b {
            path_comps.next();
            base_comps.next();
        } else {
            break;
        }
    }

    let mut result = PathBuf::new();
    for _ in base_comps {
        result.push("..");
    }
    for c in path_comps {
        if let Component::Normal(s) = c {
            result.push(s);
        }
    }
    Some(result)
}

pub fn repo_lines(name: &str, info: Option<&GitInfo>, width: usize) -> Vec<String> {
    let width = width.max(MIN_WIDTH);
    let info = match info {
        Some(i) => i,
        None => {
            return vec![fit(
                &format!(
                    "{} · not a git repository, or git is unavailable",
                    clean(name)
                ),
                width,
            )];
        }
    };

    let main = info.worktrees.iter().find(|w| w.main);
    let main_path = main.map(|w| w.path.as_str()).unwrap_or("");
    let linked: Vec<&crate::sources::gitinfo::Worktree> =
        info.worktrees.iter().filter(|w| !w.main).collect();
    let checked_out: HashSet<&str> = info
        .worktrees
        .iter()
        .filter(|w| !w.branch.is_empty())
        .map(|w| w.branch.as_str())
        .collect();
    let idle: Vec<&str> = info
        .branches
        .iter()
        .map(|b| b.as_str())
        .filter(|b| !checked_out.contains(b))
        .collect();

    let mut lines = vec![fit(
        &format!(
            "{} · {} · {}",
            clean(name),
            count(info.branches.len(), "branch", "branches"),
            count(linked.len(), "worktree", "worktrees")
        ),
        width,
    )];

    let mut rows: Vec<(&str, String, String)> = Vec::new();
    if let Some(m) = main {
        let b = clean(&m.branch);
        rows.push((
            "*",
            if b.is_empty() {
                "(detached)".to_string()
            } else {
                b
            },
            String::new(),
        ));
    }
    for w in &linked {
        let b = clean(&w.branch);
        rows.push((
            " ",
            if b.is_empty() {
                "(detached)".to_string()
            } else {
                b
            },
            worktree_label(&w.path, main_path),
        ));
    }
    for &b in idle.iter().take(MAX_IDLE_BRANCHES) {
        rows.push((" ", clean(b), String::new()));
    }

    let max_branch_len = rows
        .iter()
        .map(|(_, b, _)| UnicodeWidthStr::width(b.as_str()))
        .max()
        .unwrap_or(0);
    let col = max_branch_len.min((width / 2).max(10));

    for (marker, branch, tree) in rows {
        let fitted_branch = fit(&branch, col);
        let fitted_width = UnicodeWidthStr::width(fitted_branch.as_str());
        let padding = " ".repeat(col.saturating_sub(fitted_width));
        let left = format!("  {marker} {fitted_branch}{padding}");
        if !tree.is_empty() {
            let left_width = UnicodeWidthStr::width(left.as_str());
            let room = (width.saturating_sub(left_width + 4)).max(6);
            lines.push(format!("{left}  → {}", fit(&tree, room)));
        } else {
            lines.push(left.trim_end().to_string());
        }
    }

    if idle.len() > MAX_IDLE_BRANCHES {
        lines.push(format!(
            "    +{} more branches",
            idle.len() - MAX_IDLE_BRANCHES
        ));
    }

    lines
}

pub fn repos_text(s: &StatusSnapshot, width: usize) -> String {
    let mut blocks = vec![repo_lines("workspace", s.workspace_git.as_ref(), width)];
    for c in &s.codebases {
        if !c.exists {
            blocks.push(vec![fit(
                &format!(
                    "{} · WARNING missing target {}",
                    clean(&c.name),
                    c.path.display()
                ),
                width,
            )]);
        } else if !c.is_git {
            blocks.push(vec![fit(
                &format!("{} · not a git repository", clean(&c.name)),
                width,
            )]);
        } else {
            blocks.push(repo_lines(
                &c.name,
                s.codebase_git.get(&c.name).and_then(|x| x.as_ref()),
                width,
            ));
        }
    }

    if s.codebases.is_empty() {
        blocks.push(vec![
            "No linked codebases. `init` can link sibling repos into codebase-symlinks/."
                .to_string(),
        ]);
    }

    blocks
        .into_iter()
        .map(|b| b.join("\n"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn conflicts_text(s: &StatusSnapshot) -> String {
    if s.conflicts.is_empty() {
        return "No overlapping Affected Paths between claimed tasks.".to_string();
    }
    s.conflicts
        .iter()
        .map(|c| {
            format!(
                "WARNING {} and {} both touch {}",
                c.first,
                c.second,
                clean(&c.path)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
