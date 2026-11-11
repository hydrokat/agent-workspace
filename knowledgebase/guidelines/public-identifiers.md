# Public Identifiers Policy

## Rule 1 — Never Expose Raw Auto-Increment IDs

Auto-increment integer IDs must never appear in API responses, URLs, client-side
logs, or analytics payloads. Hash them before egress.

## Rule 2 — Hash on Egress, Decode on Ingress

- **Egress:** Before returning data to the client, encode the auto-increment ID
  into an opaque hash.
- **Ingress:** When receiving a hashed ID from the client, decode it back to the
  internal auto-increment ID before accessing the database.
- **Never persist** the hashed value. It is a transient representation derived
  from the internal ID at the boundary layer.

## Rule 3 — Per-Resource Salt

Every resource type gets its own unique salt:

```ts
// config/hash-ids.ts
import Hashids from 'hashids';
const salts = {
  user: new Hashids('user-salt-2026', 12),
  order: new Hashids('order-salt-2026', 12),
  product: new Hashids('product-salt-2026', 12),
};
export function encodeId(resource: string, id: number): string {
  return salts[resource].encode(id);
}
export function decodeId(resource: string, hash: string): number {
  return salts[resource].decode(hash)[0] as number;
}
```

Two different resource types with the same internal ID must produce different
public hashes.

## Rule 4 — UUID Preference

- For resources shared between systems (API integrations, event buses, webhooks),
  prefer UUIDv7 instead of auto-increment IDs.
- UUIDv7 is time-ordered and index-friendly.
- Do NOT hash UUIDs — they are already globally unique and opaque.

## Rule 5 — No Authorization via Public ID

A hashed ID is a lookup token, not an authorization control. Always verify
ownership or permission before serving the resource, even when the caller
provides a valid decoded ID.