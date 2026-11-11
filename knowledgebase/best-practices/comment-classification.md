# Comment Classification (Keep vs Remove)

Shared rules for `/code-cleanup` (`comments.py`), `spec-implement` Step 5, and the delivery-task cleanup pass.

## Always kept (never listed)

- Documentation-generator comments: JSDoc/TSDoc `/** */`, PHPDoc, Javadoc, rustdoc `///` and `//!`, Go doc comments on exported identifiers and the package clause, Swagger/OpenAPI annotations, Python `#:`.
- Shebangs, encoding lines, license/copyright headers (first 30 lines).
- Tool directives: `eslint-disable`, `@ts-*`, `noqa`, `type: ignore`, `pragma`, `fmt: off`, `prettier-ignore`, `istanbul ignore`, `nolint`, `rubocop:`, `shellcheck`, `go:generate`, `#region`, and similar.
- TODO/FIXME that names an owner or a ticket.

## Candidates

| Kind | Meaning |
|---|---|
| `commented-out-code` | Disabled code. Version control already holds it. |
| `restates-code` | Words repeat the adjacent line (`// increment counter` over `counter++`). |
| `narration` | Step or thinking phrasing (`Step 1:`, `let's`, `first,`). |
| `divider` / `empty` | Decoration with no content. |
| `stale-todo` | TODO/FIXME with no owner or ticket. |
| `other` | Explanatory prose. Review for a non-obvious why. Listed only on request. |

Judgment rule: keep a comment that explains a non-obvious *why*, an invariant, a compatibility constraint, or security-sensitive behavior.

## Scanner lessons

- Strings decide what is a comment. `#` or `//` inside a string, regex, template literal, heredoc, YAML block scalar, or URL is content. Test each language with these cases.
- PHP strings span lines, and inline HTML after `?>` is not code. Blade (`*.blade.php`) is not scanned.
- Rust strings span lines, and `'a` is a lifetime, not a char.
- A comment with code after it on the same line (`x /* c */ + 1`) is left alone.
- Validate against real code: remove every comment from a large repo and compare against a language oracle (`php -w`, Python token stream without `COMMENT`/`NL`, `node --check`, `ruby -c`, `bash -n`). This caught the PHP multiline-string bug that unit fixtures missed.
