# AI Development Guideline: Never Expose Auto-Increment IDs Publicly

## Core Principle

Internal auto-incrementing numeric IDs are private implementation details and must never be exposed externally.

They must not appear in:

- URLs
- API payloads
- Query parameters
- HTML attributes
- JavaScript state
- Logs visible to users
- Exported files
- Third-party integrations
- Public events / websockets

Instead, all externally visible identifiers must use hashed IDs generated from internal IDs.

---

# Mandatory Rules

## 1. Client Must Always Use Hashed IDs

All identifiers sent from frontend clients must be hashed IDs.

Examples:

Good:
- `/users/jR82Ka91`
- `/orders/Ab92LmQx`

Bad:
- `/users/15`
- `/orders/9281`

## 2. Server Must Never Return Raw IDs

Before sending any resource to clients, convert internal numeric IDs into hashed IDs.

Example Response:
```json
{
  "id": "jR82Ka91",
  "name": "John Doe"
}
```

Never:
```json
{
  "id": 15
}
```

## 3. Server Must Decode Incoming Hashed IDs

Whenever receiving an ID from client input:
- route params
- body payloads
- query filters
- nested relation references

The server must:
1. Decode the hashed ID
2. Validate ownership / authorization
3. Continue business logic using internal numeric ID

## 4. Decode Failures Must Be Safe

If decoding fails, return generic errors only.

Example:
```json
{
  "message": "Invalid identifier."
}
```

Avoid leaking:
- whether record exists
- hash algorithm details
- expected format
- database internals

Never:
```json
{
  "message": "Hash decode failed for user table using salt xyz"
}
```

## Salt Strategy

### 5. Every Resource Must Have Unique Salt

Each resource/entity must use a dedicated salt (e.g., `user`, `order`, `invoice`, `project`).

This prevents predictable cross-resource decoding. A User hash must not decode using Order salt.

### 6. Every Config Salt Must Be Combined With .env Secret

Resource salts must not be hardcoded alone.

Use: `final_salt = config(resource_salt) + env(APP_HASHID_SECRET)`

Example: `APP_HASHID_SECRET=super-long-random-secret`

This ensures environment isolation:
- local hashes differ from staging
- staging hashes differ from production

## Persistence Rules

### 7. Never Store Hashed IDs

Hashed IDs are transport-layer identifiers only.

Do NOT persist hashed IDs in:
- database columns
- foreign keys
- cache records
- audit logs
- analytics storage

Always persist internal numeric IDs.

Hashed IDs must be:
- encoded at response time
- decoded at request time

## Architectural Pattern

### Inbound Flow
`Client -> hashed_id -> decode -> internal_id -> domain logic`

### Outbound Flow
`Database internal_id -> encode -> hashed_id -> client`

## Code Standards

### Centralize Logic
Use shared services/helpers:
- `HashIdService.encode(resource, id)`
- `HashIdService.decode(resource, hash)`

Avoid duplicating hash logic across controllers.

### Type Safety
Use explicit value objects when possible (e.g., `UserPublicId`, `OrderPublicId`, `ProjectPublicId`). Avoid passing raw strings everywhere.

### Security Considerations
Hashed IDs help mitigate:
- sequential enumeration
- scraping numeric records
- guessing neighboring IDs
- leaking growth metrics

**Hashed IDs are not authorization.** Always enforce authentication, authorization, tenancy checks, and ownership checks.

## AI Coding Agent Instructions

When generating code:
1. Never expose auto-increment IDs externally.
2. Always transform internal IDs into hashed IDs in serializers/resources.
3. Always decode hashed IDs at controller boundaries.
4. Use per-resource salts.
5. Combine salts with environment secret.
6. Never store hashed IDs.
7. Fail safely on invalid decode.
8. Use internal IDs only inside repositories/services/database logic.

### Example Anti-Pattern
```php
return User::find($request->id); // If id came from client directly.
```

### Correct Pattern
```php
$internalId = HashIdService::decode('user', $request->id);

if (!$internalId) {
    abort(400, 'Invalid identifier.');
}

return User::findOrFail($internalId);
```

## Final Rule
If a user can see it, copy it, inspect it, or send it: **it must be a hashed ID, never an auto-increment ID.**
