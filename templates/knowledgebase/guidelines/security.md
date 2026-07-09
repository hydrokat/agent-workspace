# Security Guidelines

These standards apply to every codebase linked to this workspace.

## Public Identifiers

- Never expose raw auto-increment database IDs to frontend clients, public APIs,
  URLs, client-facing logs, or analytics payloads.
- Convert auto-increment IDs into opaque hashed public identifiers before
  sending data outside the backend trust boundary.
- Do not persist hashed public identifiers in the database.
- Decode hashed public identifiers at the backend boundary before accessing
  internal records.
- Treat public identifiers as opaque lookup tokens, not authorization controls.
- See `public-identifiers.md` in this directory for the full policy.

## OWASP Review

- At the start of every feature security review, check the official OWASP Top
  Ten project for the current released version.
- Use the current OWASP Top 10 as a minimum review baseline, not a full
  checklist.

## Feature Security Audit

Every feature must receive a security audit before release covering:
exposed identifiers, authentication, authorization, input validation, output
encoding, data privacy, logging, errors, dependency risk, and abuse cases.

Issues found during audit must be fixed, accepted with documented rationale,
or explicitly deferred with an owner and follow-up task.

## Security Testing in Sad Paths

Every unit test, integration test, and E2E test must cover malicious payloads
from the following categories on every user-controlled input:

- XSS and HTML injection
- SQL and NoSQL injection
- Command injection
- Path traversal
- Template injection (SSTI)
- Mass assignment / prototype pollution
- Oversized and boundary-busting payloads
- Encoding bypass attempts

Refer to `best-practices/malicious-payloads.md` for the full payload reference
and expected system behavior for each category.