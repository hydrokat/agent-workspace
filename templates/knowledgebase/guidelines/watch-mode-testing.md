# Watch-Mode Testing

## Principle

Start one background test watcher at the beginning of a development session, then
leave it running. After every edit, wait for the watcher to finish its rerun and
read only the failures it reports. Do not invoke the test runner again manually
after each change.

Rules:

- One watcher per project or test suite, started once per session.
- After an edit, wait for the watcher's rerun to complete before reading output.
- Read failures only. Never paste a full passing log into the conversation.
- Use the watcher for fast red-green feedback during TDD. Full-suite gates still
  apply before marking a task done, before committing, and in CI (see
  [Full-run gates](#full-run-gates) below).

## Rust — bacon

Install bacon (requires Rust toolchain):

```bash
cargo install --locked bacon
```

Run the built-in `test` job in headless (no-TUI) mode:

```bash
bacon --headless test
```

To use cargo-nextest instead of the standard test harness, use the built-in
`nextest` job (requires nextest installed separately):

```bash
bacon --headless nextest
```

### Exporting results to a file

Add an `[exports.locations]` block to your project's `bacon.toml` so that
diagnostics land in a file that agents can read without attaching to the terminal:

```toml
[exports.locations]
auto = true
path = ".bacon-locations"
line_format = "{kind} {path}:{line}:{column} {message}"
```

`auto = true` writes the file on every job execution. The format template
variables are `{kind}`, `{job}`, `{item-idx}`, `{path}`, `{line}`, `{column}`,
`{message}`, and `{context}`.

### Controlling watched paths

```toml
[jobs.test]
command = ["cargo", "test"]

# default_watch = true watches src/, tests/, examples/, build.rs, benches/,
# and Cargo.toml. Set to false to watch only the paths in `watch`.
default_watch = true

# Additional paths to watch beyond the defaults.
watch = ["fixtures"]

# Glob patterns for files bacon must ignore.
# Prevents the watcher from re-triggering on its own output file.
ignore = [".bacon-locations", ".bacon.socket"]

# When true (default), changes to .gitignore-excluded files do not trigger a rerun.
apply_gitignore = true
```

Add `.bacon-locations` and `.bacon.socket` to your `.gitignore` as well.

Versions checked: bacon 3.26.0 (2026-09-26). The `--headless` flag was added in
v3.8.0. The `[exports.locations]` block and all job config options above are
present in v3.23.0 docs.

## Node.js — Built-in test runner

```bash
node --test --watch \
  --test-reporter=spec --test-reporter-destination=stdout \
  --test-reporter=junit --test-reporter-destination=test-results.xml
```

`--test-reporter` and `--test-reporter-destination` pair by position. Built-in
reporter names are `spec`, `tap`, `dot`, `junit`, and `lcov`. Repeat the flags to
write to both stdout and a file simultaneously.

Note: watch mode is `Stability: 1 — Experimental` as of Node.js 22/24 LTS. The
test runner itself is `Stability: 2 — Stable` since v20.0.0.

Versions checked: Node.js 22 and 24 LTS docs on nodejs.org (2026-11-21).

## Bun

```bash
bun test --watch
```

Set `AGENT=1` or `CLAUDECODE=1` to suppress passing-test noise and show only
failure details and a summary:

```bash
AGENT=1 bun test --watch
```

Both variables are documented in Bun's official runtime-behavior docs. They
suppress output for passing, skipped, and todo tests, leaving only failures and a
final summary line.

Versions checked: Bun docs (bun.com/docs/test/runtime-behavior, 2026-11-21).

## Vitest

```bash
npx vitest --watch --reporter=json --outputFile=test-results.json
```

Vitest defaults to watch mode when running in a TTY; use `--watch` explicitly so
the watcher starts in non-TTY environments (such as an agent session) rather than
falling back to a single run. The `outputFile` option requires a reporter that
produces structured output (`json` or `junit`).

Note: as of Vitest 4 the JSON reporter writes to `.vitest/json/output.json` by
default; `--outputFile` overrides that path.

Versions checked: vitest.dev docs and vitest 4 release notes (2026-11-21).

## Jest

```bash
jest --watchAll --json --outputFile=test-results.json
```

Use `--watchAll` to rerun the entire suite on any file change. `--watch` reruns
only tests related to changed files and requires a git repository for change
detection. `--json` switches Jest's output to machine-readable JSON; all test
noise goes to stderr. `--outputFile` writes the JSON to a file (requires
`--json`).

Versions checked: jestjs.io/docs/cli (2026-11-21).

## PHP — PHPUnit and Pest

### PHPUnit with phpunit-watcher

```bash
vendor/bin/phpunit-watcher watch
```

phpunit-watcher has no built-in file export. Pass `--log-junit` through to PHPUnit
for a machine-readable file:

```bash
vendor/bin/phpunit-watcher watch --log-junit=test-results.xml
```

Flags appended after `watch` are forwarded to PHPUnit. Alternatively, set them
under the `phpunit.arguments` key in `.phpunit-watcher.yml`:

```yaml
phpunit:
  arguments: "--log-junit=test-results.xml"
```

Requirements: PHP ≥ 8.4.1, spatie/phpunit-watcher ^1.24, PHPUnit 13.

### Pest

The `pestphp/pest-plugin-watch` package was archived on 2025-08-20 and Pest's CLI
API reference does not list a `--watch` flag. Use phpunit-watcher to drive Pest in
watch mode:

```bash
vendor/bin/phpunit-watcher watch --log-junit=test-results.xml
```

Check [pestphp.com/docs/cli-api-reference](https://pestphp.com/docs/cli-api-reference)
after upgrading Pest to see whether a built-in watch flag has been added.

Versions checked: pestphp.com/docs/cli-api-reference, github.com/spatie/phpunit-watcher,
packagist.org/packages/spatie/phpunit-watcher (2026-11-21).

## Python — pytest-watcher

```bash
ptw . --now
```

`--now` runs the tests immediately on startup instead of waiting for the first
file change. Additional pytest flags are passed through:

```bash
ptw . --now -- -x --lf
```

(Flags after `--` go directly to pytest; flags before are handled by ptw.)

Reserved ptw flags: `--runner`, `--patterns`, `--ignore-patterns`, `--now`,
`--delay`, `--clear`, `--notify-on-failure`. All other flags are forwarded.

Versions checked: pytest-watcher 0.6.3 (pypi.org/project/pytest-watcher,
released 2026-01-10).

## Full-run gates

Watchers may run a subset of tests (for example, `jest --watch` only reruns tests
related to changed files). Before marking a task done, before every commit, and in
CI, run the full suite through the workspace test harness:

```bash
python3 .agents/skills/test-failures/scripts/run_tests.py -- <test command>
```

The watcher provides fast feedback during the TDD cycle. `run_tests.py` is the
authority for pass/fail at every gate.

## Hygiene

- **Stop watchers at session end.** Background watchers consume file-system events
  and CPU; always stop them when the session ends.
- **One watcher per target.** Never start a second watcher on the same project or
  suite while one is already running. Two watchers on one target produce duplicate
  runs and interleaved output.
- **Keep state files out of git.** Add `.bacon-locations`, `.bacon.socket`, and
  any reporter output files to `.gitignore`. Do not commit them.
- **Never hide failures by restarting.** If a watcher reports a failure or times
  out, treat it as a real failure. Restarting the watcher to get a clean run does
  not fix the underlying problem.
