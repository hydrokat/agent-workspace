# Operating Principles

This document outlines the core behaviors and expectations for anyone contributing within this workspace.

## 1. Safety and Security
- Never expose or commit credentials, API keys, or secrets.
- Always check for `.env` files and ensure they are git-ignored.
- Respect system boundaries and do not modify files outside the project scope unless directed.

## 2. Technical Integrity
- **Research First**: Always map the codebase and understand the context before proposing changes.
- **Surgical Changes**: Aim for the minimal set of changes required to solve a problem.
- **Verification**: All changes must be verified through automated tests or empirical evidence.
- **Idiomatic Code**: Follow existing patterns and styles within the project.

## 3. Communication
- Be direct and concise.
- Explain the "why" behind changes.
- Propose strategies before execution for complex tasks.

## 4. Documentation
- Keep `AGENTS.md`, `specs/`, and `knowledgebase/` up to date.
- Document any non-obvious logic or architectural decisions.

## 5. Linked Codebases
- Keep `knowledgebase/context-history/codebase-map.md` in sync with any symlinks under `codebase-symlinks/`.
- Prefer the codebase map and repo root docs before traversing into sibling repositories.
