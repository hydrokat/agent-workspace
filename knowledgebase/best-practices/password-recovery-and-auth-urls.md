# Password Recovery, Auth Callbacks, and Reverse-Proxy Site URL Guide

## Context & Common Failure Modes

In multi-environment deployments (local dev, Docker staging, cloud production), password reset and email invite flows are prone to subtle integration failures:

1. **Docker Environment Precedence Shadowing**: Unconditionally exporting fallback variables (e.g. `NEXT_PUBLIC_SUPABASE_URL=http://localhost:54321`) in a container entrypoint overrides disk-based `.env` files because `@next/env` never overwrites variables already present in `process.env`.
2. **Reverse Proxy Header Drift**: Reverse proxies (such as Nginx or Cloudflare) often chain headers (e.g., `X-Forwarded-Proto: https, http` or `X-Forwarded-Host: staging.example.com, 10.0.0.1`) or omit `X-Forwarded-Proto` altogether. Raw string interpolation (`${proto}://${host}`) creates malformed URLs or downgrades to `http://`, which Supabase GoTrue rejects during `resetPasswordForEmail()` as non-allowlisted `redirectTo` targets.
3. **Internal Container Origin Leaks**: When route handlers (such as `/auth/callback`) fail and redirect to `${origin}/auth/auth-code-error`, using the internal container request URL origin (`http://0.0.0.0:3000`) directs users to an unreachable address.
4. **Malformed Query Parameter Merging**: Appending query parameters as raw strings (`${next}&recovery=true`) without checking if `next` already has query parameters produces HTTP 404 paths like `/reset-password&recovery=true`.
5. **Supabase Built-in Mailer Quotas**: Supabase's default transactional email service limits sending to 2 emails per hour and only delivers to project organization members.

## Architectural Patterns

### 1. Centralized Site URL Resolution (`lib/config/site-url.ts`)

Instead of parsing `x-forwarded-*` or `host` headers ad-hoc across actions and routes:

```typescript
import { resolvePublicOrigin, sanitizeNextPath, buildAuthRedirectUrl } from '@/lib/config/site-url';

// 1. Resolve canonical origin (prioritizes NEXT_PUBLIC_SITE_URL, then validates proxy headers):
const origin = resolvePublicOrigin(requestOrHeaders);

// 2. Sanitize user-provided next paths against open redirects:
const cleanPath = sanitizeNextPath(searchParams.get('next'), '/');

// 3. Build absolute callback URL with merged query parameters:
const callbackUrl = buildAuthRedirectUrl({
  origin,
  path: '/auth/callback',
  queryParams: { next: '/reset-password?recovery=true' }
});
```

Key security controls in `resolvePublicOrigin`:
- Prioritizes `NEXT_PUBLIC_SITE_URL` / `SITE_URL` when configured, eliminating host-header injection risks entirely in production.
- Validates candidate host against strict hostname regex (rejecting `/`, `\`, `@`, whitespace, control characters, CRLF).
- Takes the first hop of comma-separated proxy headers.
- Forces `https://` for non-loopback hosts in `NODE_ENV === 'production'` even if reverse proxies omit `X-Forwarded-Proto`.
- Safely falls back from container bind address (`0.0.0.0`) to the browser's `origin` header.

### 2. Open Redirect Prevention (`sanitizeNextPath`)

All redirect targets supplied via user input or query parameters (`next`) must be sanitized:
- Rejects protocol-relative URLs (`//evil.com`).
- Rejects absolute schemes (`https://evil.com`, `javascript:`, `data:`).
- Rejects backslash bypass attempts (`\evil.com` or `/\evil.com`).
- Defaults to `/` or the provided fallback path.

### 3. Rate-Limit Normalization in Server Actions

Surface actionable rate-limit messages to users without leaking database or service credentials:

```typescript
if (error) {
  throw actionError(
    "auth.requestPasswordReset",
    error,
    "Failed to send password reset email.",
    [
      /security purposes/i,
      /rate limit/i,
      /after \d+ seconds/i,
    ],
    {
      redirect_origin: origin,
      error_code: error.code,
      error_status: error.status,
    }
  );
}
```

## Production Deployment Checklist

1. **Set `NEXT_PUBLIC_SITE_URL`**: Set to the canonical HTTPS domain (e.g. `https://tracker.hackthenorth.ph`) in production/staging environment secrets.
2. **Configure Supabase Redirect URLs**:
   - In Supabase Dashboard -> **Authentication -> URL Configuration -> Redirect URLs**, add:
     - `https://<domain>/auth/**`
     - `http://localhost:3000/auth/**` (dev only)
3. **Enable Custom SMTP**:
   - In Supabase Dashboard -> **Project Settings -> Authentication -> SMTP Settings**, connect a custom SMTP provider (Resend, SendGrid, Amazon SES) to eliminate the 2 emails/hour restriction.
