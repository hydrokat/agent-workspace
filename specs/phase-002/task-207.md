# Task 207: Final UI Polish for Phase 2

## Objective
Apply the Nexus design system (`knowledgebase/guidelines/design/DESIGN.md`) to the Phase 2 pages to ensure strict adherence to the brand guidelines.

## Target Pages
- `/dashboard`
- `/tasks`
- `/admin/teams`
- `/admin/tasks`

## Requirements
- **Razor-sharp corners**: Ensure all elements use `rounded-none`.
- **Correct hex colors**:
  - Base: `#050505`
  - Surface: `#0a0a0a`
  - Primary: `#c70000`
- **High-contrast typography**:
  - Font: Space Grotesk
  - Headers: bold/black (`#ffffff`)
  - Metadata: muted grey (`#888888`)
- **Grid layouts and borders as spacing**: Use 1px solid borders (`#222222`) instead of whitespace to define zones.
- **Subtle red glow effects**: Apply `box-shadow: 0 0 15px rgba(199, 0, 0, 0.2)` on active/primary elements.
- **No regressions**: All tests must pass after the UI changes.

## Tasks
1. **Update `globals.css` and `tailwind.config.ts` (if necessary)**: Ensure the base colors and font are correctly set up.
2. **Polish `/dashboard`**: Apply the design system to the member dashboard.
3. **Polish `/tasks`**: Apply the design system to the task discovery page.
4. **Polish `/admin/teams`**: Apply the design system to the admin teams management page.
5. **Polish `/admin/tasks`**: Apply the design system to the admin task management page.
6. **Run Tests**: Ensure all existing tests pass.

## Agent Assignment
- `frontend-dev`: Implement the UI changes.
- `qa-engineer`: Run tests and verify no regressions.
- `code-reviewer`: Review the code for adherence to the design system and code quality.
