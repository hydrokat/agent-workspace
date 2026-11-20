---
name: cleanup-tests
description: Audits, cleans up, improves, and strengthens automated tests. Use /cleanup-tests when explicitly invoked or whenever the task involves cleaning up, reviewing, refactoring, or improving existing tests.
---

# /cleanup-tests — Test Quality & Security Audit

## Objective

Ensure that the project's automated tests are meaningful, maintainable, reliable, and comprehensive.

This skill is responsible for identifying weak, redundant, incomplete, misleading, and missing tests, then improving them without introducing imaginary functionality or changing application behavior unnecessarily.

The objective is not to maximize test count or coverage percentages. The objective is to establish confidence that the application behaves correctly under normal, unexpected, and malicious conditions.

## 1. Inspect Before Modifying

Before making changes:

1. Inspect the existing test structure and testing framework.
2. Review the application's existing implementation and architecture.
3. Identify the actual components, services, integrations, endpoints, and business rules.
4. Review existing test conventions, factories, fixtures, helpers, and utilities.
5. Identify redundant, incomplete, outdated, and meaningless tests.
6. Determine which application behaviors are not adequately tested.

Do not assume functionality exists simply because a test references it.

Do not modify application behavior just to make an existing test pass.

## 2. Code Cleanup

Clean up existing tests by:

- Removing unnecessary comments and commented-out code.
- Removing duplicate and redundant tests.
- Removing unused imports, variables, fixtures, and helpers.
- Consolidating repetitive setup where appropriate.
- Avoiding excessive abstraction and unnecessary test utilities.
- Following existing project naming and organizational conventions.
- Keeping tests readable and independently understandable.

Comments should only remain when they explain non-obvious behavior or important testing decisions.

## 3. Required Test Coverage

For every relevant feature, operation, or business rule, evaluate the following categories.

### 3.1 Happy Paths

Test successful operations:

- Valid inputs produce expected results.
- Successful requests return the correct response.
- Data is persisted correctly.
- Business rules are applied correctly.
- Related records and side effects are handled correctly.
- Successful operations return only the expected information.

### 3.2 Sad Paths

Test unsuccessful operations:

- Invalid inputs.
- Missing required fields.
- Nonexistent resources.
- Unauthorized requests.
- Forbidden operations.
- Invalid state transitions.
- Failed dependencies.
- Database or transaction failures where applicable.

Verify that failures produce the expected behavior and do not leave the application in an inconsistent state.

### 3.3 Edge Cases

Identify and test relevant boundary conditions:

- Null and empty values.
- Minimum and maximum allowed values.
- Zero and negative numbers.
- Extremely large inputs.
- Duplicate records.
- Concurrent or repeated operations.
- Missing relationships.
- Unexpected data types.
- Boundary dates and timezones.
- Empty collections and large collections.

Only test edge cases that are relevant to the actual implementation and business rules.

### 3.4 Security and Malicious Paths

Test how the application handles potentially malicious behavior.

Consider:

- SQL injection.
- Cross-site scripting (XSS).
- Path traversal.
- Command injection.
- Mass assignment.
- IDOR (Insecure Direct Object Reference).
- Privilege escalation.
- Unauthorized resource access.
- Malicious file uploads.
- Unexpected or manipulated request payloads.
- Parameter tampering.
- Replay or repeated requests where relevant.

Verify that malicious inputs are rejected, safely handled, or rendered harmless through the appropriate security controls.

Never assume that frontend validation is sufficient.

### 3.5 Error Handling

Verify that the application handles errors correctly:

- Appropriate HTTP status codes.
- Consistent error response structures.
- Meaningful validation messages.
- No accidental exposure of stack traces or internal implementation details.
- Proper exception handling.
- Correct transaction rollback behavior.
- No partial or corrupted data after failed operations.
- Appropriate logging without exposing sensitive information.

### 3.6 Data Leakage Prevention

Verify that only the necessary data is exposed or transmitted.

Inspect:

- API responses.
- Serialized resources.
- Database queries where relevant.
- Events and event payloads.
- Job payloads.
- Notifications.
- Logs.
- External API requests.
- File exports.

Ensure that sensitive or unnecessary information is not exposed, including:

- Passwords and password hashes.
- Access tokens and secrets.
- Internal identifiers that should not be exposed.
- Private user information.
- Internal database attributes.
- Unnecessary relationships and nested data.

Test both the presence of required fields and the absence of forbidden fields.

### 3.7 Input Validation and Sanitization

Verify that inputs are handled safely before reaching sensitive operations.

Test:

- Whitespace normalization.
- Unexpected characters.
- HTML and script payloads.
- Malformed strings.
- Invalid encodings where applicable.
- Unexpected nested objects.
- Disallowed fields.
- Oversized payloads.
- Invalid file types and content.

Ensure that validation, normalization, escaping, and sanitization are applied at the correct boundaries.

Do not silently alter legitimate data unless the application's requirements explicitly call for it.

### 3.8 Authorization and Data Isolation

Verify that users cannot access or manipulate resources outside their permitted scope.

Test:

- Role-based permissions.
- Resource ownership.
- Organizational hierarchy restrictions.
- Tenant isolation.
- Cross-user resource access.
- Cross-tenant data access.
- Restricted state transitions.
- Privileged operations.

Test authorization at the backend boundary, not just through frontend visibility.

### 3.9 Regression Coverage

When fixing a bug:

1. Identify the original failure.
2. Write a test that reproduces the problem.
3. Confirm that the test fails before the fix when practical.
4. Apply the fix.
5. Confirm that the test passes.
6. Verify that related existing behavior remains intact.

Every meaningful bug fix should have a corresponding regression test whenever feasible.

## 4. Test Integrity Rules

These rules are mandatory.

**Never introduce:**

- Blank tests.
- Tests containing no meaningful assertions.
- Tests that only execute code without verifying behavior.
- Tests for imaginary components, services, endpoints, or integrations.
- Tests that mock functionality that does not exist.
- Assertions that are guaranteed to pass regardless of application behavior.
- Excessive mocks that bypass the actual behavior being tested.
- Tests that duplicate existing coverage without additional value.
- Tests that depend on execution order.
- Tests that pass only because of hardcoded or fabricated assumptions.

**Mocks and fakes are allowed** when they isolate genuine external dependencies or make tests deterministic. However, they must represent real interfaces and actual application boundaries.

Prioritize testing observable behavior over internal implementation details.

## 5. Execution Workflow

Follow this sequence:

### Step 1 — Audit
Inspect the existing tests and implementation. Identify weaknesses, gaps, and redundant coverage.

### Step 2 — Plan
Prepare a concise list of:
- Tests to remove.
- Tests to improve.
- Missing test scenarios.
- Security and data-leakage risks.
- Relevant regression scenarios.

### Step 3 — Clean Up
Remove unnecessary code and improve existing tests without changing application behavior unnecessarily.

### Step 4 — Strengthen Coverage
Add meaningful tests for missing happy paths, sad paths, edge cases, security scenarios, error handling, and data protection.

### Step 5 — Execute
Run the relevant test suite.

Fix failing tests caused by the changes. Investigate unrelated failures rather than hiding, skipping, or weakening them.

### Step 6 — Verify
Verify that:
- All new tests exercise real functionality.
- Assertions validate actual behavior.
- No unnecessary mocks or imaginary dependencies were introduced.
- Existing functionality remains intact.
- Relevant security scenarios are covered.
- Tests are deterministic and maintainable.

### Step 7 — Report
Provide a concise summary of:
- Tests removed or cleaned up.
- Tests added or improved.
- Security and coverage gaps addressed.
- Test execution results.
- Remaining gaps or known limitations.

## 6. Definition of Done

The task is complete only when:

- [ ] Unnecessary comments and dead test code are removed.
- [ ] Existing tests are meaningful and relevant.
- [ ] Happy paths are covered.
- [ ] Sad paths are covered.
- [ ] Relevant edge cases are covered.
- [ ] Relevant malicious paths are covered.
- [ ] Error handling is verified.
- [ ] Data leakage risks are tested.
- [ ] Input validation and sanitization are tested.
- [ ] Authorization and data isolation are tested where applicable.
- [ ] Regression scenarios are covered.
- [ ] No blank or meaningless tests remain in the reviewed scope.
- [ ] Tests follow existing project conventions.
- [ ] Relevant test suites pass.

Not every category needs an artificial test for every feature. Apply coverage based on actual functionality, risk, and applicable security boundaries. If a scenario is not applicable, do not invent one just to satisfy a checklist.

## 7. Final Principles

1. Test real behavior, not imaginary functionality.
2. Prefer meaningful assertions over high test counts.
3. Prefer realistic integration tests when they provide stronger confidence than isolated mocks.
4. Never weaken application security to make tests pass.
5. Never remove a meaningful test simply because it is difficult to maintain.
6. Do not modify production behavior unless a verified defect requires it and the change is within scope.
7. Do not skip or suppress failing tests to achieve a green test suite.
8. Treat security, data isolation, and information exposure as first-class testing concerns.
9. Follow the existing project's architecture and conventions.
10. A passing test suite is not proof of correctness if the tests do not validate meaningful behavior.
