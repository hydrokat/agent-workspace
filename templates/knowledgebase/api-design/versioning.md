# API Versioning Policy

## One Rule

> If existing clients can continue working without modification, keep the
> current version. If existing clients would break or behave differently,
> create a new version.

Versioning is driven by **contract compatibility**, not by implementation
changes or release cadence.

## Keep the Current Version — Non-Breaking Changes

These changes do not require clients to update:

### Add new endpoints

```
GET /api/v1/products
GET /api/v1/products/{id}

↓

GET /api/v1/products
GET /api/v1/products/{id}
GET /api/v1/products/{id}/inventory
```

Existing clients are unaffected.

### Add optional request fields

Before `POST /api/v1/products`:

```json
{ "name": "Laptop" }
```

After:

```json
{ "name": "Laptop", "barcode": "123456789" }
```

If `barcode` is optional, existing clients continue working.

### Add optional response fields

Before:

```json
{ "id": 1, "name": "Laptop" }
```

After:

```json
{ "id": 1, "name": "Laptop", "category": "Electronics" }
```

Well-designed clients ignore unknown fields.

### Add optional query parameters

```
GET /products?page=1
↓
GET /products?page=1&category=laptop
```

If the new parameter is optional, keep v1.

### Performance improvements

Caching, database optimization, queue processing, infrastructure migration
(MySQL → PostgreSQL), internal refactoring — never a version change.

### Fix bugs

A fix that aligns behavior with the documented contract stays in the same
version. If clients relied on buggy behavior, document the change prominently
but do not bump the version.

## Create a New Version (Breaking Changes)

These changes require clients to modify their code:

| Change | Examples | New version? |
|---|---|---|
| Remove a field | Drop `description` from response | Yes |
| Rename a field | `customerName` → `fullName` | Yes |
| Change field type | `"price": 100` → `"price": "100.00"` | Yes |
| | `"status": 1` → `"status": "ACTIVE"` | Yes |
| Make optional field required | `phone` was optional, now required | Yes |
| Change endpoint semantics | `DELETE /products` now archives instead of deletes | Yes |
| Remove an endpoint | `GET /orders/history` no longer exists | Yes |
| Change pagination format | Offset/page → cursor-based | Yes |
| Change auth model | API Key → OAuth2 | Usually yes |

### Remove fields

Before:

```json
{ "id": 1, "name": "Laptop", "description": "..." }
```

After:

```json
{ "id": 1, "name": "Laptop" }
```

### Rename fields

```json
{ "customerName": "John" }
```

↓

```json
{ "fullName": "John" }
```

### Change field types

```
"price": 100          →    "price": "100.00"
"status": 1           →    "status": "ACTIVE"
```

### Make optional fields required

```json
{ "email": "..." }   →   { "email": "...", "phone": "0917..." }
```

## Gray Areas

These changes are technically compatible but can still affect consumers:

- Sorting defaults
- Default page size
- Timezone handling
- Decimal precision
- Stricter validation
- Rate limiting

They usually do not justify a new version but must be documented, announced,
and rolled out gradually if impactful.

## Version Lifecycle

| Phase | Behavior |
|---|---|
| Active | Full support. Bugs fixed, non-breaking improvements accepted. |
| Deprecated | No new features. Critical bugs only. Sunset date announced. |
| Sunset | Returns 410 Gone. Clients must migrate before this date. |

- Keep the deprecated version for a minimum of **6–12 months**.
- Communicate migration paths before sunsetting.
- Support both versions during the deprecation window when feasible.

## URL Convention

```
/api/v1/products
/api/v2/products
```

Do not use sub-version URL segments (`/v1.1`, `/v1.2`). Track implementation
releases internally with Git tags or semver (e.g., `v1.8.4`, `v1.9.0`).

## Decision Tree

| Change | New version? |
|---|---|
| Add endpoint | No |
| Add optional field | No |
| Add optional query parameter | No |
| Improve performance | No |
| Fix bugs | Usually no |
| Rename field | Yes |
| Remove field | Yes |
| Change data type | Yes |
| Remove endpoint | Yes |
| Make optional field required | Yes |
| Change response structure | Yes |
| Change auth model | Usually yes |
| Change business meaning of endpoint | Yes |