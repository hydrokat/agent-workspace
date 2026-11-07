#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

RUN_TESTS=""
if [ -f "$ROOT/../.agents/skills/test-failures/scripts/run_tests.py" ]; then
  RUN_TESTS="python3 $ROOT/../.agents/skills/test-failures/scripts/run_tests.py --"
elif [ -f "$ROOT/.agents/skills/test-failures/scripts/run_tests.py" ]; then
  RUN_TESTS="python3 $ROOT/.agents/skills/test-failures/scripts/run_tests.py --"
fi

run_cmd() {
  local desc="$1"
  shift
  printf "==> %s...\n" "$desc"
  if [ -n "$RUN_TESTS" ]; then
    $RUN_TESTS "$@"
  else
    "$@"
  fi
}

echo "=== Running check-all regression gate ==="

# 1. Cargo fmt
run_cmd "Cargo format check" cargo fmt --manifest-path "$ROOT/tandem/Cargo.toml" --check

# 2. Cargo clippy
run_cmd "Cargo clippy check" cargo clippy --manifest-path "$ROOT/tandem/Cargo.toml" --all-targets -- -D warnings

# 3. Cargo test in strict mode
export TANDEM_TEST_STRICT=1
run_cmd "Cargo test (strict mode)" cargo test --manifest-path "$ROOT/tandem/Cargo.toml" --locked

# 4. npm test
run_cmd "Node installer tests" npm --prefix "$ROOT" test

# 5. pytest test/skills
PYTEST_CMD=""
if [ -x "$ROOT/test/skills/.venv/bin/pytest" ]; then
  PYTEST_CMD="$ROOT/test/skills/.venv/bin/pytest"
elif command -v pytest >/dev/null 2>&1; then
  PYTEST_CMD="pytest"
elif python3 -m pytest --version >/dev/null 2>&1; then
  PYTEST_CMD="python3 -m pytest"
fi

if [ -n "$PYTEST_CMD" ]; then
  run_cmd "Pytest skills test suite" $PYTEST_CMD "$ROOT/test/skills"
else
  echo "Warning: pytest not available, skipping test/skills"
fi

# 6. Coverage comparison against floor
if command -v cargo-llvm-cov >/dev/null 2>&1; then
  run_cmd "Tandem crate coverage floor check" cargo llvm-cov --manifest-path "$ROOT/tandem/Cargo.toml" --fail-under-lines 82 --summary-only
fi

echo "=== All checks passed! ==="
