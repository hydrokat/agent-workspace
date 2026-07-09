# Error Handling Patterns

## Application Errors

Define typed error classes per domain. Each error carries a machine-readable
code, a human-readable message, and optional context.

```ts
export class DomainError extends Error {
  constructor(
    public readonly code: string,
    message: string,
    public readonly context?: Record<string, unknown>,
  ) {
    super(message);
    this.name = 'DomainError';
  }
}
```

## Boundary Translation

Translate domain errors to HTTP responses (or transport-specific errors) at
the boundary layer, never in the domain:

- `NotFoundError` → 404
- `UnauthorizedError` → 401
- `ForbiddenError` → 403
- `ValidationError` → 422
- `ConflictError` → 409
- `RateLimitError` → 429

## Unhandled Errors

A global error handler at the outermost layer catches unhandled errors, logs
them with a correlation ID, and returns a generic 500 response. Never expose
internal error details to the client in production.