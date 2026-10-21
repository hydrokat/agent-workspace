# {{task_id}}: Delivery: Commit, PR, Review, CI, Merge

## Description
Finalize the phase by committing the completed work, creating a pull request, resolving review feedback, waiting for pipelines, fixing failures, and merging only after review and pipeline gates pass.

## Branches
- **Working Branch**: {{branch}}
- **Merge Target**: {{target}}

## Objectives
- [ ] Confirm all implementation tasks in this phase are complete.
- [ ] Review final diff and exclude unrelated changes.
- [ ] Confirm each requirement is classified as `New Feature`, `Enhancement`, or `Bug Fix`.
- [ ] Confirm code structure diagrams and existing codebase guide notes are current.
- [ ] Confirm regression tests or validation cover affected existing behavior, and no regression has been introduced.
- [ ] Run a final cleanup pass before code review with `/code-cleanup --diff {{target}}` (list, ask, then delete): remove unnecessary comments, keep code self-documenting where practical, and preserve only useful documentation-generator comments such as JSDoc, PHPDoc, Swagger/OpenAPI, or equivalent.
- [ ] Commit changes using the `commit` skill.
- [ ] Create a pull request targeting the merge branch using the `pull-request` skill.
- [ ] Run `code-review`, including regression risk and cleanup/self-documenting-code checks.
- [ ] If code review requests changes, run `code-review-remediation`, update the PR, and repeat code review until it passes.
- [ ] Wait for required pipelines/checks to finish.
- [ ] If any required pipeline/check fails, fix the failure, update the PR, and repeat until required pipelines/checks pass.
- [ ] Merge the pull request into the selected merge target.
- [ ] If the agent workspace itself is a git repository (`git rev-parse --show-toplevel` equals the workspace root), deliver it too: commit the workspace changes (specs, knowledgebase, `AGENTS.md`, skills) on a branch with the `commit` skill, never on `main`/`master`/`trunk`. If it has a remote, open a PR and merge it after review. If it has no remote, stop after the branch commit and ask the user how to integrate it.
- [ ] Record each repository delivered (codebases and, when applicable, the agent workspace) with its branch and PR/commit in this task's notes.

## Status
- [ ] Pending
