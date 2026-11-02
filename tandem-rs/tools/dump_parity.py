#!/usr/bin/env python3
"""Generate deterministic golden parity fixtures from Python tandem 1.5.0.

Run with:
    uv run --project tandem python ../tandem-rs/tools/dump_parity.py
"""
import dataclasses
import json
import os
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

# Ensure reproducible time and timezone
os.environ["TZ"] = "UTC"
if hasattr(time, "tzset"):
    time.tzset()

# Locate tandem package
ROOT = Path(__file__).resolve().parent.parent.parent
TANDEM_SRC = ROOT / "tandem" / "src"
FIXTURES_DIR = Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "parity"
FIXTURES_DIR.mkdir(parents=True, exist_ok=True)

if str(TANDEM_SRC) not in sys.path:
    sys.path.insert(0, str(TANDEM_SRC))

import tandem
from tandem import conversations, notice, quota, state, update_check, updater, versions
from tandem.adapters import agy, base, claude
from tandem.coordination import schema
from tandem.sources import codebases, conflicts, inprogress, specs
from tandem.status import render
from tandem.tmux import client, keys, layout, tabs


def to_dict(obj: Any) -> Any:
    if dataclasses.is_dataclass(obj):
        return {k: to_dict(v) for k, v in dataclasses.asdict(obj).items()}
    if isinstance(obj, (list, tuple)):
        return [to_dict(x) for x in obj]
    if isinstance(obj, dict):
        return {str(k): to_dict(v) for k, v in obj.items()}
    if isinstance(obj, Path):
        return str(obj)
    return obj


def write_fixture(name: str, data: Any) -> None:
    path = FIXTURES_DIR / f"{name}.json"
    cleaned = to_dict(data)
    rendered = json.dumps(cleaned, indent=2, sort_keys=True) + "\n"
    path.write_text(rendered, encoding="utf-8")
    print(f"Wrote {path.relative_to(ROOT)}")


def dump_keys() -> None:
    shortcuts = [
        "Alt+h", "Alt+1", "Alt+2", "Alt+q", "Ctrl+c", "Shift+Up", "Alt+Shift+Left",
        "Alt++", "Alt+-", "Ctrl+Alt+z"
    ]
    keymap_dict = [
        {
            "key": b.key, "group": b.group, "hint": b.hint,
            "description": b.description, "command": b.command
        }
        for b in keys.KEYMAP
    ]
    valid_override = keys.keymap({"close-agent": "Ctrl+w", "quit": "Alt+x"})
    empty_override = keys.keymap({})

    data = {
        "default_keymap": keymap_dict,
        "display_shortcut": {
            "darwin": {s: keys.display_shortcut(s, platform="darwin") for s in shortcuts},
            "linux": {s: keys.display_shortcut(s, platform="linux") for s in shortcuts},
        },
        "option_char": {
            "darwin": {s: keys.option_char(s, platform="darwin") for s in shortcuts},
            "linux": {s: keys.option_char(s, platform="linux") for s in shortcuts},
        },
        "hint_bar": {
            "darwin": keys.hint_bar(platform="darwin"),
            "linux": keys.hint_bar(platform="linux"),
        },
        "hint_bar_with_ranges": {
            "darwin": keys.hint_bar_with_ranges(platform="darwin"),
            "linux": keys.hint_bar_with_ranges(platform="linux"),
        },
        "help_lines": {
            "darwin": keys.help_lines(platform="darwin"),
            "linux": keys.help_lines(platform="linux"),
        },
        "wizard_help_lines": {
            "darwin": keys.wizard_help_lines(platform="darwin"),
            "linux": keys.wizard_help_lines(platform="linux"),
        },
        "bind_commands": {
            "darwin": keys.bind_commands(platform="darwin", python="/usr/bin/python3"),
            "linux": keys.bind_commands(platform="linux", python="/usr/bin/python3"),
        },
        "mouse_commands": keys.mouse_commands(python="/usr/bin/python3"),
        "resize_commands": keys.resize_commands(python="/usr/bin/python3"),
        "overrides": {
            "valid": [
                {"key": b.key, "group": b.group, "hint": b.hint, "description": b.description, "command": b.command}
                for b in valid_override
            ],
            "empty": [
                {"key": b.key, "group": b.group, "hint": b.hint, "description": b.description, "command": b.command}
                for b in empty_override
            ],
        }
    }
    write_fixture("keys", data)


def dump_tmux() -> None:
    sample_paths = [
        "/tmp/project",
        "/Users/alice/Projects/workspace",
        "/home/bob/dev/my-app",
        "/var/repo/deep/nested/path",
    ]
    sample_panes = [
        tabs.PaneInfo("%1", "tandem", "agent", "planner", 1, "claude·opus", False, None, "s-1", "spec1", "1.0"),
        tabs.PaneInfo("%2", "tandem", "agent", "planner", 2, "agy·sonnet", True, 0, "s-2", "spec2", "1.0"),
        tabs.PaneInfo("%3", "tandem", "agent", "implementer", 1, "claude·sonnet", False, None, "s-3", "spec3", "1.0"),
    ]
    data = {
        "socket_for": {p: client.socket_for(Path(p)) for p in sample_paths},
        "conf_text": {
            "default": layout.conf_text(),
            "history_10000": layout.conf_text(10000),
        },
        "tabs": {
            "tab_text": [tabs.tab_text(p) for p in sample_panes],
            "render_title_planner": tabs.render_title(sample_panes, "planner"),
            "render_title_implementer": tabs.render_title(sample_panes, "implementer"),
        }
    }
    write_fixture("tmux", data)


def dump_adapters() -> None:
    clis = ["claude", "agy"]
    roles = ["planner", "implementer"]
    models = ["opus", "sonnet", "gemini-2.5-pro"]
    resumes = [False, True]
    skips = [False, True]
    conversations_list = [None, "c0a80101-0000-0000-0000-000000000001"]
    log_files = [None, "/tmp/tandem-test.log"]

    cases = []
    for cli_name in clis:
        adapter = claude.ClaudeAdapter(self_cmd=["tandem"]) if cli_name == "claude" else agy.AgyAdapter()
        for role in roles:
            for model in models:
                for resume in resumes:
                    for skip in skips:
                        for conv in conversations_list:
                            for log in log_files:
                                spec = base.AgentSpec(cli_name, role, model, 1, skip, "/workspace", conv)
                                try:
                                    argv = adapter.build_argv(spec, resume=resume, log_file=log)
                                    error = None
                                except Exception as e:
                                    argv = None
                                    error = f"{type(e).__name__}: {e}"
                                cases.append({
                                    "cli": cli_name, "role": role, "model": model,
                                    "resume": resume, "skip_permissions": skip,
                                    "conversation": conv, "log_file": log,
                                    "argv": argv, "error": error
                                })

    data = {
        "statusline_settings": {
            "default": claude.statusline_settings(),
            "custom": claude.statusline_settings(self_cmd=["/usr/local/bin/tandem"]),
        },
        "build_argv_matrix": cases,
        "claude_models": [m.id for m in claude.ClaudeAdapter().list_models()],
        "agy_models_sample": [
            m.id for m in agy.parse_models("gemini-2.5-pro\tGemini 2.5 Pro\nclaude-3-5-sonnet\tSonnet 3.5\n")
        ]
    }
    write_fixture("adapters", data)


def dump_state() -> None:
    agents = (
        base.AgentSpec("claude", "planner", "opus", 1, False, "/w", "c0a80101-0000-0000-0000-000000000001"),
        base.AgentSpec("agy", "implementer", "gemini-2.5-pro", 1, True, "/w", None),
    )
    s = state.SavedState(agents, {"planner": 1, "implementer": 1}, 50, 10, 1700000000.0, True)

    good_json = state.to_json(s)
    partial_json = {
        "version": 2,
        "saved_at": 1700000000.0,
        "agents": [
            {"cli": "claude", "role": "planner", "model": "opus", "index": 1, "conversation": "c0a80101-0000-0000-0000-000000000001"}
        ]
    }
    bad_json_version = {"version": 99, "agents": []}
    bad_json_no_agents = {"version": 2, "agents": []}

    now = 1700001000.0
    data = {
        "to_json": good_json,
        "parse": {
            "good": state.to_json(state.parse(good_json)),
            "partial": state.to_json(state.parse(partial_json)),
            "bad_version": state.parse(bad_json_version) is None,
            "bad_no_agents": state.parse(bad_json_no_agents) is None,
        },
        "describe": {
            "clean_16min": state.describe(s, now=now),
            "unclean_120min": state.describe(
                state.SavedState(agents, {"planner": 1}, None, None, now - 7200, clean=False),
                now=now
            ),
            "zero_saved_at": state.describe(
                state.SavedState(agents, {}, None, None, 0.0, clean=True),
                now=now
            ),
        }
    }
    write_fixture("state", data)


def dump_quota() -> None:
    now = 1700000000.0
    payload_full = {
        "rate_limits": {
            "five_hour": {"used_percentage": 35.0, "resets_at": int(now + 3600)},
            "seven_day": {"used_percentage": 80.0, "resets_at": int(now + 86400 * 2)},
        }
    }
    payload_five = {
        "rate_limits": {
            "five_hour": {"used_percentage": 12.0, "resets_at": int(now + 1800)},
        }
    }
    payload_invalid = {
        "rate_limits": {
            "five_hour": {"used_percentage": -10.0, "resets_at": 0},
        }
    }

    q_full = quota.parse_statusline(payload_full, now)
    q_five = quota.parse_statusline(payload_five, now)
    q_invalid = quota.parse_statusline(payload_invalid, now)

    tmux_test_strings = [
        "Normal text",
        "Text with # hash",
        "Text with #(command)",
        "Text with #[fg=red] color",
        "Control \x00\x1b\x7f chars",
        "Line with \n newline",
    ]

    data = {
        "parse_statusline": {
            "full": quota._to_json(q_full) if q_full else None,
            "five_only": quota._to_json(q_five) if q_five else None,
            "invalid": q_invalid is None,
            "empty": quota.parse_statusline({}, now) is None,
        },
        "format_line": {
            "claude_fresh": quota.format_line(["claude"], q_full, now),
            "claude_stale": quota.format_line(["claude"], q_full, now + 1000),
            "claude_and_agy": quota.format_line(["claude", "agy"], q_full, now),
            "agy_only": quota.format_line(["agy"], None, now),
            "none": quota.format_line([], None, now),
        },
        "tmux_safe": {s: quota.tmux_safe(s) for s in tmux_test_strings},
    }
    write_fixture("quota", data)


def dump_render() -> None:
    now = 10000.0
    sample_task_done = specs.Task("t-001", "Initial setup", "Done")
    sample_task_prog = specs.Task("t-002", "Core logic", "In Progress")
    sample_phase = specs.Phase("p-001-core", [sample_task_done, sample_task_prog])

    sample_session = schema.Session(
        "implementer-1", "claude", "implementer", "sonnet", "%2", "/workspace",
        "feat/core", "/workspace", "p-001 t-002", now - 15.0
    )
    sample_claim = schema.Claim(
        "p-001-core", "t-002", "implementer-1", "feat/core", "/workspace", ("src/core.rs",), now - 10.0
    )
    sample_conflict = conflicts.Conflict("t-002", "t-003", "src/core.rs")

    snap = render.StatusSnapshot(
        now=now, coord_exists=True, phases=[sample_phase],
        sessions=[sample_session], claims=[sample_claim], conflicts=[sample_conflict]
    )

    fit_strings = [
        "short",
        "this is a much longer line that might need truncation depending on width",
        "exact-width-40-characters-padded-here!",
    ]

    data = {
        "summary_line": {
            "active": render.summary_line(snap),
            "empty": render.summary_line(render.StatusSnapshot(now=now, coord_exists=True)),
        },
        "session_row": {
            "active": render.session_row(snap, 0),
        },
        "in_progress_text": {
            "active": render.in_progress_text(snap),
            "empty": render.in_progress_text(render.StatusSnapshot(now=now, coord_exists=True)),
        },
        "phases_text": {
            "active": render.phases_text(snap),
            "empty": render.phases_text(render.StatusSnapshot(now=now, coord_exists=True)),
        },
        "conflicts_text": {
            "active": render.conflicts_text(snap),
            "empty": render.conflicts_text(render.StatusSnapshot(now=now, coord_exists=True)),
        },
        "fit": {
            s: {
                "40": render.fit(s, 40),
                "80": render.fit(s, 80),
                "120": render.fit(s, 120),
            }
            for s in fit_strings
        },
        "bar": {
            "1_of_2_w4": render.bar(1, 2, 4),
            "0_of_0_w4": render.bar(0, 0, 4),
            "3_of_3_w6": render.bar(3, 3, 6),
        },
        "age": {
            "12s": render.age(12),
            "840s": render.age(840),
            "7200s": render.age(7200),
        }
    }
    write_fixture("render", data)


def dump_sources() -> None:
    fixture_specs = ROOT / "tandem" / "tests" / "fixtures" / "specs"
    phases = specs.load_phases(fixture_specs) if fixture_specs.exists() else []

    inprog_content = """# In Progress

## Example

### p-002-payments-api — t-003: Add refresh token rotation
- **Agent:** `@senior-backend-dev` (ID: `abc-1234`)

### p-001-thing — t-002: Real work
- **Agent:** `@senior-backend-dev` (ID: `sess-1`)
- **Location:** Branch `feat/x`
- **Ongoing:** Writing code.
- **Next step:** Test.
"""
    with tempfile.NamedTemporaryFile("w") as tf:
        tf.write(inprog_content)
        tf.flush()
        inprog = inprogress.parse_in_progress(Path(tf.name))

    claims_list = [
        schema.Claim("p-001", "t-001", "s1", "b1", "/w", ("src/main.rs",), 100.0),
        schema.Claim("p-001", "t-002", "s2", "b2", "/w", ("src/main.rs", "src/lib.rs"), 100.0),
    ]
    confl = conflicts.find_conflicts(claims_list)

    data = {
        "phases": [
            {
                "name": p.name,
                "done": p.done,
                "total": p.total,
                "tasks": [{"id": t.id, "title": t.title, "status": t.status} for t in p.tasks],
                "warning": p.warning,
            }
            for p in phases
        ],
        "inprogress": [
            {"spec": e.spec, "title": e.title, "fields": e.fields}
            for e in inprog
        ],
        "conflicts": [
            {"first": c.first, "second": c.second, "path": c.path}
            for c in confl
        ]
    }
    write_fixture("sources", data)


def dump_versions() -> None:
    version_inputs = [
        "1.5.0", "v1.5.0", "1.5", "2.0.0-beta.1", "tandem 1.5.0 (c24e10642)",
        "v0.9.12-pre", "invalid", "", "1.2.3.4"
    ]
    is_at_least_pairs = [
        ("1.5.0", "1.4.0"),
        ("1.5.0", "1.5.0"),
        ("1.4.9", "1.5.0"),
        ("2.0.0", "1.99.9"),
        ("1.5.0a", "1.5.0b"),
    ]
    sample_min_versions = """# Minimum versions required
TANDEM_MIN_NODE=18.0.0
TANDEM_MIN_PYTHON=3.10.0
TANDEM_MIN_TMUX=3.2
"""
    with tempfile.NamedTemporaryFile("w") as tf:
        tf.write(sample_min_versions)
        tf.flush()
        min_vers = versions.parse_min_versions(Path(tf.name))

    data = {
        "parse_version": {
            v: to_dict(versions.parse_version(v)) for v in version_inputs
        },
        "is_at_least": {
            f"{a}>={b}": versions.is_at_least(versions.parse_version(a), versions.parse_version(b))
            for a, b in is_at_least_pairs
        },
        "parse_min_versions": min_vers,
    }
    write_fixture("versions", data)


def dump_update_check() -> None:
    now = 100000.0
    interval = 14400
    due_cases = [
        {"last": 0.0, "now": now, "interval": interval, "expected": update_check.due(update_check.CheckState(last=0.0), now, interval)},
        {"last": now - 3600, "now": now, "interval": interval, "expected": update_check.due(update_check.CheckState(last=now - 3600), now, interval)},
        {"last": now - 20000, "now": now, "interval": interval, "expected": update_check.due(update_check.CheckState(last=now - 20000), now, interval)},
        {"last": now + 1000, "now": now, "interval": interval, "expected": update_check.due(update_check.CheckState(last=now + 1000), now, interval)},
    ]
    offer_cases = [
        {"installed": "1.4.0", "latest": "1.5.0", "skip": "", "expected": update_check.should_offer("1.4.0", "1.5.0", "")},
        {"installed": "1.5.0", "latest": "1.5.0", "skip": "", "expected": update_check.should_offer("1.5.0", "1.5.0", "")},
        {"installed": "1.4.0", "latest": "1.5.0", "skip": "1.5.0", "expected": update_check.should_offer("1.4.0", "1.5.0", "1.5.0")},
        {"installed": "1.4.0", "latest": "1.6.0", "skip": "1.5.0", "expected": update_check.should_offer("1.4.0", "1.6.0", "1.5.0")},
    ]
    data = {
        "due": due_cases,
        "should_offer": offer_cases,
    }
    write_fixture("update_check", data)


def dump_notice() -> None:
    sample_changelog = """# Changelog

## [1.5.0] - 2026-11-01
- **Major Rust port foundations**: initial work.
- Second bullet item without bold.
"""
    data = {
        "whats_new_headline_bold": notice._headline("- **Major Rust port foundations**: initial work."),
        "whats_new_headline_period": notice._headline("- Some sentence. More text."),
        "whats_new_text": notice.whats_new(sample_changelog, "1.4.0", "1.5.0"),
    }
    write_fixture("notice", data)


def dump_conversations() -> None:
    valid_inputs = [
        "c0a80101-0000-0000-0000-000000000001",
        "C0A80101-0000-0000-0000-000000000001",
        "invalid-uuid",
        "",
        "c0a80101000000000000000000000001",
        "c0a80101-0000-0000-0000-00000000000g",
    ]
    agy_log_sample = (
        "2026-11-01 info: starting agent\n"
        "2026-11-01 info: Created conversation c0a80101-0000-0000-0000-000000000002\n"
        "2026-11-01 info: running message\n"
    )
    with tempfile.NamedTemporaryFile("w") as tf:
        tf.write(agy_log_sample)
        tf.flush()
        found_id = conversations.from_agy_log(Path(tf.name))

    with tempfile.NamedTemporaryFile("w") as tf_empty:
        found_empty = conversations.from_agy_log(Path(tf_empty.name))

    data = {
        "valid_id": {v: conversations.valid_id(v) for v in valid_inputs},
        "from_agy_log_sample": found_id,
        "from_agy_log_empty": found_empty,
    }
    write_fixture("conversations", data)


def dump_coordination() -> None:
    query_fields = {
        "id": "q-1234",
        "status": "open",
        "author": "implementer-1",
        "target": "user",
        "created": 1700000000.0,
        "question": "Which architecture pattern?",
        "blocking": ["src/main.rs"],
        "answer": "",
        "task": "p-001 t-002",
    }
    encoded = schema.encode_query_fields(query_fields)
    decoded = schema.decode_query_fields(encoded)

    raw_claim = {
        "phase": "p-001", "task": "t-002", "session": "s-1",
        "branch": "feat/x", "worktree": "/w", "paths": ["src/lib.rs"],
        "claimed": 1700000000.0, "heartbeat": 1700000010.0, "extra_field": "ignored"
    }
    raw_session = {
        "id": "s-1", "cli": "claude", "role": "implementer", "model": "sonnet",
        "pane": "%1", "cwd": "/w", "branch": "feat/x", "worktree": "/w",
        "task": "p-001 t-002", "heartbeat": 1700000010.0,
    }
    raw_query = {
        "id": "q-1", "status": "open", "asked_by": "implementer-1", "task": "p-001 t-002",
        "blocking_scope": "src/main.rs", "created": 1700000000.0, "question": "Pattern?",
        "options": ["A", "B"], "answer": "", "answered_via": ""
    }

    data = {
        "encode_query_fields": encoded,
        "decode_query_fields": decoded,
        "claim_from_dict": to_dict(schema.Claim.from_dict(raw_claim)),
        "session_from_dict": to_dict(schema.Session.from_dict(raw_session)),
        "query_from_dict": to_dict(schema.Query.from_dict(raw_query)),
    }
    write_fixture("coordination", data)


def dump_updater() -> None:
    agent = updater.StaleAgent(
        pane_id="%1", role="planner", index=1, cli="claude", model="opus",
        launched="1.4.0", installed="1.5.0", resumable=True, busy=False, claim=None
    )
    data = {
        "describe_stale": updater.describe_stale(agent),
        "plan_frozen": updater.plan(Path("/workspace"), "1.5.0", frozen=True),
        "plan_not_frozen": updater.plan(Path("/workspace"), "1.5.0", frozen=False),
    }
    write_fixture("updater", data)


def main() -> None:
    dump_keys()
    dump_tmux()
    dump_adapters()
    dump_state()
    dump_quota()
    dump_render()
    dump_sources()
    dump_versions()
    dump_update_check()
    dump_notice()
    dump_conversations()
    dump_coordination()
    dump_updater()
    print("Done dumping all 13 golden parity fixtures.")


if __name__ == "__main__":
    main()
