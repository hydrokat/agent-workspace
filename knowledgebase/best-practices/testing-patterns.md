# Testing Patterns and Conventions

## Test Structure

Organize tests to mirror the source tree. Group by module, then by scenario:

```
tests/
  unit/
    services/
      user-service.test.ts
      order-service.test.ts
    controllers/
      user-controller.test.ts
  integration/
    user-registration.test.ts
  e2e/
    auth-flow.test.ts
    checkout-flow.test.ts
```

Each test file follows the **happy-first** convention: happy path tests first,
then sad path grouped by category (invalid input, malicious payloads,
authorization, missing data).

## Factory Pattern

Use factory functions to create test data. Factories produce **realistic**
payloads by default and allow overrides per test:

```ts
function buildUser(
  overrides: Partial<UserProps> = {},
  options: { includeMalicious?: boolean } = {},
): User {
  return {
    id: 1,
    email: 'alice@acme.com',
    name: 'Alice Chen',
    role: 'member',
    ...overrides,
  };
}
```

Override for sad paths:

```ts
// Invalid input
buildUser({ email: 'not-an-email' });

// Malicious payload
buildUser({ name: '<script>alert(1)</script>' });
```

## Mock Strategy

- Mock at the boundary (interface/port level), not on concrete implementations.
- Prefer in-memory test doubles over mocks for repositories.
- Use a spy or stub for external services that cannot run in-process.
- Always verify the mock was called with the expected arguments.

## Testing Pattern by Concern

### Services — Business Logic

- **Happy:** Call with valid data; assert return value and side effects.
- **Sad invalid:** Call with missing/wrong fields; assert domain error.
- **Sad malicious:** Call with injection payloads in every string field;
  assert the system rejects or sanitizes.
- **Sad authorization:** Call without required permissions; assert
  authorization error.

### Controllers / Handlers

- **Happy:** Send a well-formed request; assert status code, response body
  shape, and headers.
- **Sad invalid:** Send malformed JSON, missing required fields, wrong types.
- **Sad malicious:** Send XSS in body fields, SQLi in query params, path traversal
  in route params, oversized payloads.
- **Sad missing:** Request a non-existent resource (404), perform an
  unauthorized action (403).

### Repositories / Data Access

- **Happy:** Persist a valid entity; assert it can be retrieved.
- **Sad:** Query by non-existent ID; assert `null` or `NotFound`.
- **Sad malicious:** Attempt SQLi via query parameters; verify query is
  parameterized and injection is harmless.

## E2E Testing

Apply the same categories at the integration boundary:

1. **Happy:** Complete a full user journey (register → login → create resource
   → verify resource → delete resource).
2. **Sad invalid:** Submit invalid forms at each step.
3. **Sad malicious:** Inject XSS, SQLi, command injection, path traversal at
   every user-facing entry point.
4. **Sad authorization:** Access protected routes without a token, with an
   expired token, with a token for a different user.

## Write Tests First

Before writing any implementation code:

1. Write the test that describes the expected behavior.
2. Run it — verify it fails (red).
3. Write the minimal implementation (green).
4. Refactor.

Never commit implementation code without a corresponding test. Never mark a
test as passing without seeing it fail first.

## Blast Radius During Refactoring

Before refactoring any code:

1. Run the **full test suite** to establish a green baseline.
2. Identify all callers and consumers of the code being changed. Every
   dependent is part of the blast radius.
3. Make the change. Run the **full suite again**.
4. If any existing test fails without modification, the change has an
   unintended side effect — revert and re-scope.
5. For shared utilities used across multiple modules, add dedicated tests
   for each known call site pattern before modifying the utility.