# agent-workspace

A CLI that turns any project directory into an AI-ready workspace — pre-loaded with specialized agents, reusable skills, and shared guidelines, configured for whichever AI provider you use.

## What it does

When you work with AI coding assistants (Claude Code, Antigravity CLI, Codex), each conversation starts cold. The assistant has no knowledge of your team's standards, no understanding of what each role should do, and no shared guidelines to follow. You end up re-explaining the same things repeatedly.

`agent-workspace` solves that by giving you a structured directory that your AI provider reads automatically:

- **Agent definitions** — named roles (`tech-lead-orchestrator`, `senior-backend-dev`, `qa-engineer`, etc.) with clear responsibilities, delegation rules, and the right model pre-assigned per provider
- **Reusable skills** — step-by-step instructions the AI follows for recurring tasks (workspace init, knowledge management, spec planning, pull requests, code review, and `/workspace-cleanup` to find and remove files that don't belong in the workspace)
- **Shared guidelines** — architecture standards, security rules, and best practices the AI respects across every conversation, including branch protection (agents never commit directly to `main`, `master`, or `trunk` unless explicitly told to, and use a worktree or a new branch before implementing) and concurrent agent awareness (agents check for other agents' in-progress work before starting instead of assuming exclusive access)
- **Live progress tracking** — `specs/in-progress.md` lists every task or spec an agent is actively working on, its next step, and the owning spec (or `adhoc` if it has none), so you always know what's in flight; entries are removed as soon as the task completes
- **Provider outputs** — each provider gets its own compatibility format (`.claude/`, `.gemini/`) auto-generated from a single source of truth

Run one command. Get a workspace that works with Claude, Antigravity, or Codex out of the box.

## Quick start

No installation. Run directly with `npx`:

```bash
cd your-project-agent-workspace
npx hydrokat/agent-workspace init
```

The CLI will ask for your project name, which providers to configure, and whether to link any sibling codebases. Answer the prompts and you're done.

To skip prompts (CI or scripted setup):

```bash
npx hydrokat/agent-workspace init --yes --providers claude --name "my-project"
```

## Commands

### `init` — set up a new workspace

Run this once in a project directory that doesn't have a workspace yet.

```bash
npx hydrokat/agent-workspace init
```

What it does:
1. Asks for project name, providers, and whether to link sibling codebases
2. Creates the directory structure (`knowledgebase/`, `specs/`, `.agents/`)
3. Installs all agents, skills, and guidelines
4. Generates provider-specific outputs for each selected provider
5. Runs `doctor` to verify everything is correct
6. Writes `.agent-workspace/manifest.json` to remember your setup

**If the directory already has a workspace**, init will warn you and suggest running `update` instead.

Example — set up for Claude and Codex, non-interactively:

```bash
npx hydrokat/agent-workspace init --yes --providers claude,codex --name "payments-api"
```

---

### `update` — pull in the latest content

Run this when you want to get new or updated agents, skills, or guidelines from the package.

```bash
npx hydrokat/agent-workspace update
```

What it does:
- Refreshes all managed files (agents, skills, guidelines) with the latest shipped versions
- If you edited a managed file locally, it backs it up to `<file>.bak` before overwriting
- Refreshes the model table and operating principles in `AGENTS.md` (the managed block between `<!-- managed:start -->` and `<!-- managed:end -->`)
- Fully regenerates provider outputs, fixing any manual drift
- **Never touches** your project content: `specs/`, `knowledgebase/business-flows/`, or any knowledgebase entries you wrote yourself

`update` reads your saved manifest so it remembers which providers you configured — no re-prompting required.

---

### `doctor` — verify the workspace

Run this any time you want to confirm the workspace is in a valid state.

```bash
npx hydrokat/agent-workspace doctor
```

What it checks:
- Required directories and `AGENTS.md` exist
- Every agent has a generated output file for each configured provider
- Generated agents have valid frontmatter (`name`, `description`, `model`)
- Each agent's `model:` value matches the expected model for that provider
- Compatibility symlinks (`CLAUDE.md`, `GEMINI.md`) are correct
- Codebase symlinks resolve

Exits with code `0` if everything passes, non-zero if anything fails — useful in CI.

```bash
# Machine-readable output for CI pipelines
npx hydrokat/agent-workspace doctor --json
```

---

## Flags

| Flag | Commands | Description |
|---|---|---|
| `--yes` | `init`, `update` | Skip all prompts and accept defaults |
| `--providers <list>` | `init`, `update` | Comma-separated: `claude`, `antigravity`, `codex` |
| `--name <name>` | `init` | Set the project name without prompting |
| `--no-link` | `init` | Skip sibling codebase linking |
| `--link-all` | `init` | Link all sibling codebases without prompting |
| `--dry-run` | `init`, `update` | Show what would change without writing anything |
| `--quiet` | all | Suppress per-file output; show only summaries |
| `--verbose` | all | Show all actions, including unchanged files |
| `--json` | all | Emit structured JSON output (one record per line) |
| `--version` | — | Print the package version |
| `--help` | — | Print usage |

---

## What gets created

After running `init`, your project directory will contain:

```
your-project-agent-workspace/
├── AGENTS.md                          # Workspace rules (edit the overview; the rest is managed)
├── WORKFLOW.md                        # Shared execution style for all agents
├── .agent-workspace/
│   └── manifest.json                  # Tracks providers, version, managed files
├── .agents/
│   ├── agents/
│   │   ├── tech-lead-orchestrator/
│   │   ├── senior-backend-dev/
│   │   ├── frontend-dev/
│   │   ├── qa-engineer/
│   │   ├── security-auditor/
│   │   └── ...                        # 12 agents total
│   └── skills/
│       ├── specs-planner/
│       ├── workspace-init/
│       ├── workspace-sync/
│       └── ...
├── .claude/                           # Generated — do not edit manually
│   ├── agents/                        # Claude-compatible agent files (model injected)
│   └── skills/
├── .gemini/                           # Generated — do not edit manually (if configured)
│   ├── agents/
│   └── skills/
├── knowledgebase/
│   ├── guidelines/                    # Architecture, security, testing standards
│   ├── best-practices/                # Operating principles
│   ├── business-flows/                # Your project-specific flows (never touched by update)
│   └── context-history/
├── specs/                             # Your implementation plans (never touched by update)
│   └── in-progress.md                 # Live list of active tasks/specs, per-task next steps
└── .tmp/                              # Agent working files — safe to delete anytime
```

---

## Providers

Each provider gets its own format generated from the same source agents:

| Provider | Where agents live | Workspace doc | Skills location | Model behavior |
|---|---|---|---|---|
| **Claude** | `.claude/agents/*.md` | `CLAUDE.md` → symlink to `AGENTS.md` | `.claude/skills/` | `tools:` field removed; `model:` injected |
| **Antigravity** | `.gemini/agents/*.md` | `GEMINI.md` → symlink to `AGENTS.md` | `.agents/skills/` (shared) | `tools: ["*"]` added; `model:` injected |
| **Codex** | `.agents/agents/*/AGENT.md` | `AGENTS.md` (native) | `.agents/skills/` (shared) | `model:` injected in place |

The correct model is automatically assigned per agent per provider — for example, `tech-lead-orchestrator` gets `claude-opus-4-7` for Claude, `gemini-2.5-pro` for Antigravity, and `gpt-5.5` for Codex.

Antigravity reads skills from `.agents/skills/` directly (the canonical source), so no separate skills directory is created for it.

---

## Editing your workspace

### Things you should edit

- **`AGENTS.md`** — set your project name, system architecture, and directory overview in the section above `<!-- managed:start -->`. The block between the managed markers is refreshed on every `update`; content outside it is yours.
- **`WORKFLOW.md`** — customize the shared execution style for your team.
- **`knowledgebase/business-flows/`** — add process flows, decision paths, and operational context specific to your project.
- **`knowledgebase/context-history/`** — record important architectural decisions or major changes.
- **`specs/`** — create implementation plans and task breakdowns here.

### Things you should not edit manually

- **`.claude/`** and **`.gemini/`** — these are fully regenerated on every `update`. Any manual edits will be overwritten (with a `.bak` backup). (`.gemini/` is the agent output directory for Antigravity.)
- **`.agents/agents/`** and **`.agents/skills/`** — these are managed library files. If you edit them locally, `update` will back them up and restore the shipped version.

If you want to add a custom agent or skill that won't be overwritten, the right place is a separate directory outside `.agents/` (e.g., `my-agents/`), then reference it from your `AGENTS.md`.

---

## Sibling codebase linking

If you work across multiple repositories, `init` can create symlinks to sibling directories (projects at the same level in your filesystem) under `codebase-symlinks/`. It also generates a `codebase-map.md` that agents read before navigating between repos.

```bash
# Init will prompt: "Link sibling codebases? [y/N]"
npx hydrokat/agent-workspace init

# Or link all siblings automatically
npx hydrokat/agent-workspace init --link-all

# Or skip linking entirely
npx hydrokat/agent-workspace init --no-link
```

---

## Workflow for day-to-day use

```
1. init once          →  npx hydrokat/agent-workspace init
2. open your AI tool  →  Claude Code, Antigravity CLI (`agy`), Codex, etc.
3. agents are ready   →  the AI reads .claude/ or .gemini/ automatically
4. update when needed →  npx hydrokat/agent-workspace update
5. verify any time    →  npx hydrokat/agent-workspace doctor
```

---

## Contributing

1. Edit canonical files only: `AGENTS.md`, `.agents/agents/`, `.agents/skills/`, `templates/`.
2. Never manually edit generated files under `.gemini/` or `.claude/`. (`.gemini/` contains Antigravity agent files.)
3. After editing base files, run `npx . update` to regenerate outputs.
4. To add a new agent or skill, add it under `templates/agents/` or `templates/skills/` and update `src/data/models.json` with its model assignments.
5. Run `npm test` before submitting.

## License

MIT
