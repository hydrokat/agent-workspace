# Testing and Knowledgebase Guidelines

## TDD First — Write Tests Before Code

Every implementation must start with a failing test. Follow the
red-green-refactor cycle:

1. **Red:** Write the test. Run it — it must fail.
2. **Green:** Write the minimal code to pass the test.
3. **Refactor:** Improve code quality without changing behavior.

Do not write implementation code before the test exists. A test that has never
failed is not a valid test.

## Refactoring — Preserve Existing Tests

When refactoring, existing tests are the safety net that defines a refactor:

- **A refactor must not change test outcomes.** Every existing test must pass
  before and after the change, with no modifications to the test itself.
- If an existing test must be updated, the change is a redesign or a feature
  addition, not a refactor. Treat it as a new feature: write the test first.
- **Blast radius:** Before extracting a function, renaming a variable, or
  restructuring a module, run the full test suite. Then make the change. Run the
  full suite again. If any test fails, the change is either incorrect or has
  an unexpected side effect — revert and re-scope.

## Unit Testing — Happy Path and Sad Path

Every unit test suite must cover:

### Happy Path

- Test the expected success flow with a **realistic payload**.
- Use real-world-shaped data: valid emails, realistic string lengths,
  proper UUIDs, meaningful business values.
- Verify the return value, side effects, and state after success.

```ts
// Good — realistic payload
const user = await registerUser({
  email: 'alice@acme.com',
  name: 'Alice Chen',
  password: 'valid-P@ssword-1',
});
expect(user.email).toBe('alice@acme.com');
expect(user.id).toBeDefined();
```

```ts
// Bad — placeholder junk
const user = await registerUser({ email: 'a@b.co' });
```

### Sad Path — Invalid Input

- Test every validation rule: missing fields, wrong types, boundary values,
  malformed formats.
- Verify the correct error type, status code, and error message.

```ts
it('rejects empty email', async () => {
  await expect(
    registerUser({ email: '', password: 'valid-P@ssword-1' })
  ).rejects.toThrow(ValidationError);
});
```

### Sad Path — Malicious Payloads

- Test injection attacks: SQLi, NoSQLi, XSS, template injection,
  command injection in every user-controlled string field.
- Test mass assignment / prototype pollution in object fields.
- Test path traversal in file or URL fields.
- The system must reject or safely sanitize these payloads without crashing.

```ts
it('rejects XSS in name field', async () => {
  await expect(
    registerUser({
      email: 'test@test.com',
      name: '<script>alert("xss")</script>',
      password: 'valid-P@ss1',
    })
  ).rejects.toThrow(ValidationError);
});
```

```ts
it('rejects SQLi in email field', async () => {
  await expect(
    registerUser({
      email: "' OR 1=1 --",
      password: 'valid-P@ss1',
    })
  ).rejects.toThrow(ValidationError);
});
```

### Sad Path — Authorization and Missing Data

- Test unauthenticated access, forbidden access, and resource-not-found
  scenarios.
- Test rate-limiting and idempotency key rejection when applicable.

## E2E Testing

The same rules apply to end-to-end tests:

- **Happy path** with realistic payloads through the full system flow.
- **Sad path** with invalid input at every integration boundary.
- **Sad path** with malicious payloads (XSS, injection, traversal) at
  every user-facing entry point.
- E2E tests should cover the critical user journeys — registration,
  authentication, primary CRUD flows, payment — and mirror the
  unit test sad path categories at the integration boundaries.

## Coverage Target

- Minimum **85%** line, branch, and function coverage.
- Run coverage reports in CI. Fail the build if coverage drops below
  the threshold.
- Do not count test infrastructure (factories, fixtures, config) toward
  coverage.

## Knowledgebase Updates

Update the knowledgebase after every significant change:

| Event | Where to document |
|---|---|
| Bug fix with reusable lesson | `best-practices/` or `guidelines/` |
| New feature or behavior | `business-flows/`, `api-design/`, or `db-schema/` |
| Architectural decision | `architecture-decisions/` with date and context |
| Task completion | `context-history/task-completion-log.md` |
| Lesson learned | `best-practices/lessons-learned.md` |

Prefer focused files in the appropriate subdirectory over adding unrelated
notes to a catch-all document.