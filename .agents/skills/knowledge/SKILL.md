---
name: knowledge
description: Use when the knowledgebase needs to be added to, updated, reorganized, or synced with current project decisions. Trigger with /knowledge to update guidelines, best-practices, business-flows, and context-history as needed.
metadata:
  category: documentation
  triggers: knowledge, knowledgebase, context-history, best-practices, business-flows, guidelines
---

# Knowledge

This skill governs the structure and maintenance of the shared project intelligence in `/knowledgebase/`.

Use this skill when the user invokes `/knowledge` or when a task requires updates to the workspace knowledgebase.

## Collaboration Model

Let `business-analyst` and `technical-writer` collaborate on knowledgebase work:
- `business-analyst` is responsible for clarifying requirements, documenting business intent, defining flows, and identifying what knowledge should be captured.
- `technical-writer` is responsible for turning confirmed information into clear, consistent, well-structured documentation and cross-references.

Use both roles together when updating the knowledgebase:
1. `business-analyst` determines what changed and what should be documented.
2. `technical-writer` writes or refines the documentation so it is discoverable, accurate, and consistent.

## Directory Targets

- **[`/guidelines`](../../knowledgebase/guidelines/)**: Operational standards, policies, rules, and stable guidance.
- **[`/best-practices`](../../knowledgebase/best-practices/)**: Reusable patterns, lessons learned, and recommended approaches.
- **[`/business-flows`](../../knowledgebase/business-flows/)**: Business processes, user flows, decision paths, and operational context.
- **[`/context-history`](../../knowledgebase/context-history/)**: Important historical context, decision rationale, major changes, and resolved complexity.

## Workflow

### 1. Search First
Before writing anything:
1. Search the existing knowledgebase for related material.
2. Reuse and update existing files when possible instead of creating duplicates.
3. Check `AGENTS.md`, `WORKFLOW.md`, and relevant `specs/` files for context that should be reflected in the knowledgebase.

### 2. Decide What To Update
For each request or project change, determine which of these sections should be updated accordingly:
- `guidelines`
- `best-practices`
- `business-flows`
- `context-history`

Do not assume only one section needs changes. Review all four and update the relevant ones.

### 3. Capture Content With Shared Ownership
When the knowledgebase needs updating:
1. Have `business-analyst` define the problem statement, flows, assumptions, and business or product implications.
2. Have `technical-writer` structure the content, normalize terminology, and produce the final documentation updates.
3. Keep the resulting documentation aligned with the current implementation and workspace rules.

### 4. Record Changes In The Right Place
- Add or update `guidelines/` when the change affects standards or operating rules.
- Add or update `best-practices/` when the change creates a reusable pattern or lesson.
- Add or update `business-flows/` when the change affects process, workflow, or decision logic.
- Add or update `context-history/` when the change is historically important, architectural, or useful for future reasoning.

## Quality Standards

- **High-Signal Only**: Focus on what changed, why it matters, and how it should be applied.
- **Atomic Files**: Prefer focused documents over large catch-all notes.
- **Cross-Referencing**: Link to relevant `specs/`, `AGENTS.md`, and `WORKFLOW.md` entries when helpful.
- **Consistency**: Keep terminology aligned across all knowledgebase sections.
- **Current-State First**: Document the current truth, not outdated plans.

## Mandates

- **Trigger Name**: This skill should be easy to invoke with `/knowledge`.
- **Read First**: Always search the knowledgebase before making updates.
- **Collaborate By Role**: Use `business-analyst` and `technical-writer` together for meaningful knowledgebase updates.
- **Update Accordingly**: Update `context-history`, `best-practices`, `business-flows`, and `guidelines` as needed for the task, not just the first matching section.
- **Keep It Clean**: If the knowledgebase becomes cluttered, reorganize it into clearer files or categories.
