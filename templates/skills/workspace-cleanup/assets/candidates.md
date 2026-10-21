## Workspace Cleanup Candidates

### OS/editor cruft
- `path/to/file` — reason, last modified

### Stale backups
- `path/to/file.bak` — reason, last modified

### Orphaned generated output
- `.claude/skills/old-skill/` — no matching `.agents/skills/old-skill/`

### Empty directories
- `path/to/dir/` — empty, not documented in AGENTS.md

### Unrecognized files
- `path/to/file` — not covered by Core Directory Map or .gitignore

### Stale scratch files
- `.tmp/path` — older than 14 days, not referenced by `specs/in-progress.md`, last modified

### Needs review
- `path` — why it is unclear
