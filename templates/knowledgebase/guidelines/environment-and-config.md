# Environment and Config Access Policy

## Rule 1 — Never Read process.env Directly

Code must never access `process.env` (Node), `std::env` (Rust), `os.Getenv` (Go),
or equivalent at the call site. Access env through a typed config module or struct.

**Good:**
```ts
import { env } from './env';
export const dbConfig = {
  host: env.string('DB_HOST', 'localhost'),
  port: env.integer('DB_PORT', 5432),
};
```

**Bad:**
```ts
const host = process.env.DB_HOST ?? 'localhost';
```

## Rule 2 — Config Module Interface

The config module must expose typed accessors:

| Accessor | Returns | Default support |
|---|---|---|
| `string(key, default?)` | `string` | Yes |
| `integer(key, default?)` | `number` | Yes |
| `boolean(key, default?)` | `boolean` | Yes |
| `secret(key)` | `string` | No (must be set) |

Secret accessors must never log or expose the value in error messages.

## Rule 3 — Environment Boundaries

- **Development:** Only `local` env may be accessed. No AI agent may access
  staging, production, or any non-local environment unless explicitly allowed
  in the task prompt.
- **CI/CD:** The CI pipeline injects env through the runner's secret store, not
  through checked-in `.env` files.
- **Production:** Env is injected at deploy time via the orchestration layer.
  Config values must have sensible defaults for local dev so the app boots
  without a full env file.

## Rule 4 — .env Files

- `.env` files must be in `.gitignore`.
- A `.env.example` file may be checked in with placeholder values and comments
  explaining each variable.
- Never commit a `.env` file containing real secrets to version control.