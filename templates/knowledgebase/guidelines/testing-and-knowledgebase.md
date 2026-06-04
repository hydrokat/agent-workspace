# Testing and Knowledgebase Guidelines

These standards define the minimum delivery bar for features, fixes, and completed tasks.

## Unit Testing

- Every feature must include unit tests for the happy path.
- Every feature must include unit tests for sad paths, including invalid input,
  authorization failure, missing data, boundary values, and expected downstream failure.
- Bug fixes must include a regression test that fails before the fix when the codebase has
  a test framework capable of expressing the case.
- If automated tests are unavailable, document the manual validation steps and the reason
  automation was not possible.

## Knowledgebase Updates

- Every time a bug is fixed, update the knowledgebase with the cause, fix, and prevention note
  when the lesson is reusable.
- Every time a feature is released, update the knowledgebase with new behavior, assumptions,
  operational notes, or business context.
- Every time a task is completed, update the knowledgebase with the resulting status, decision,
  or lesson learned.
- If no more specific knowledgebase file applies, record the completion in
  `knowledgebase/context-history/task-completion-log.md`.
- Prefer focused files under `guidelines/`, `best-practices/`, `business-flows/`, or
  `context-history/` instead of adding unrelated notes to a catch-all document.
