# Deterministic Skill Scripts

Skills keep the judgment work (choosing, explaining, deciding). Anything that is the same every time (numbering, parsing, status sync, scanning, file discovery, deletion) runs as a script.

## Contract

- **Language**: Python 3, stdlib only. No new runtime dependencies.
- **Invocation**: `python3 .agents/skills/<skill>/scripts/<name>.py <command> …`. Files copied by `init` and `update` lose their executable bit, so never rely on `./script.py`.
- **Output**: JSON on stdout for every read. Errors are `{"error": "...", ...}` on stdout.
- **Exit codes**: `0` ok, `1` conflict / drift / not found, `2` invalid input.
- **Arguments**: pass as separate quoted values, never interpolate into a shell string. Validate ids with anchored regexes (`^p-\d{3}-[a-z0-9-]+$`, `^t-\d{3}$`) and keep every resolved path inside the expected root.
- **Tolerant reads**: a missing section comes back as `null` plus a `warnings[]` entry. A parser must not crash on hand-written files.
- **Writes**: temp file + rename. Refuse to overwrite existing content.

## Fallback

`SKILL.md` names the script and also documents the manual steps. If the script is missing or fails, the skill says so and does the manual steps. A script failure never blocks the task.

## Layout

```text
.agents/skills/<skill>/
├── SKILL.md
├── scripts/<name>.py     # deterministic steps
├── assets/*.md           # templates; {{field}} filled by the script, [prose] filled by the agent
└── agents/openai.yaml
```

A skill that owns a format owns the script for it. Other skills call that script by path (`spec-implement` calls `spec-planner/scripts/specs.py`).

## When not to script

Do not script judgment: code review, commit grouping and wording, requirement classification, explanations, feasibility verdicts. Script the parts around them: finding the files, parsing the specs, counting, rendering a template, checking for drift, deleting what was approved.

## Templates in `assets/`

- A template is a Markdown file in the skill's `assets/`. The script renders it with a strict renderer: every `{{field}}` in the template must have a value, and a missing value is an error (exit 1), not a blank.
- `{{field}}` marks what the script knows (numbers, titles, branches, lists). `[prose]` marks what the agent writes.
- A `lint` command closes the loop: it fails while a `{{field}}` or any `[placeholder]` from the shipped templates is left in a generated file. Text inside backticks or fenced code is ignored.
- Scaffold commands render everything first, then write: new files before the file that lists them, each through temp file + rename.
- Insert-and-renumber commands refuse when the structure is already wrong (for example the delivery task is not last), instead of guessing.

## Approval-gated deletion

A script that deletes must not decide what to delete:

1. `scan` writes its candidates (with size and mtime, or the exact text) to a saved state file and prints them.
2. The agent shows them, the user approves, and the agent passes only the approved items back.
3. The script re-checks every item against the saved scan (listed, unchanged, inside the root, not protected, not reached through a symlink) and refuses on any mismatch.
4. Protected areas are enforced in the delete step itself, so a forged or stale state file cannot reach them.

## Lessons

- Test against real, hand-written files, not only the templates. A `## Status` line can be free text, so status resolution falls back to the plan's Timeline label.
- Never find a Markdown section with `str.partition("## Status")` or a bare regex. The heading text also appears in prose and in code fences. Scan lines, skip fenced blocks, and require the heading at the start of a line.
- `argparse` treats a leading `-` argument as an option and exits 2 with usage on stderr, not JSON. Tests for invalid ids should not use `-x`-style values.
