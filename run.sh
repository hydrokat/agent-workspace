#!/usr/bin/env bash
# tandem bootstrap: check dependencies, offer installs/updates, enforce minimum versions, launch tandem.
#
# Exit codes: 0 ok, 2 declined install, 3 declined/failed required update, 4 install/update/config failure,
#             5 unsupported platform, 6 prompt needed but stdin is not a terminal.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TANDEM_DIR="$ROOT_DIR/.agent-workspace/tandem"
[ -d "$TANDEM_DIR" ] || TANDEM_DIR="$ROOT_DIR/tandem"
MIN_FILE="$TANDEM_DIR/min-versions.env"
TOOLS="git tmux uv claude agy"
AI_TOOLS="claude agy"
CLAUDE_INSTALL_URL="https://claude.ai/install.sh"
AGY_INSTALL_URL="https://antigravity.google/cli/install.sh"
UV_INSTALL_URL="https://astral.sh/uv/install.sh"

export HOMEBREW_NO_AUTO_UPDATE=1
export HOMEBREW_NO_ENV_HINTS=1
TANDEM_STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/tandem"
VENV_SLUG="$(printf '%s' "$TANDEM_DIR" | tr -c 'A-Za-z0-9' '-')"
VENV_SLUG="${VENV_SLUG#-}"
export UV_PROJECT_ENVIRONMENT="${UV_PROJECT_ENVIRONMENT:-$TANDEM_STATE_DIR/venv-$VENV_SLUG}"

FETCH_RUNTIME=0
RUNTIME=""
RUNTIME_REASON=""
EXP_VER=""
SLUG=""
MODE=""
ANSWERS=""
ANSWERS_SET=0
CHECK_ONLY=0
NO_UPDATE=0
ORIG_ARGS=()
PASSTHRU=()
MIN_DATA=""
PM=""
CAP_OUT=""
CMD=()

usage() {
  cat <<'USAGE'
Usage: ./run.sh [options] [-- tandem-args...]

Checks git, tmux, uv, claude, and agy, asks before installing or updating anything,
then launches tandem from the current directory.

Options:
  --check          Report status only; change nothing, launch nothing.
  --yes            Answer yes to every prompt.
  --no             Answer no to every prompt.
  --answers y,n,.. Answer prompts in order from a list (automation); missing answers mean no.
  --no-update      Skip the launch-time workspace self-update check.
  -h, --help       Show this help.

Environment:
  TANDEM_AUTO_UPDATE=0   Disable launch-time auto-update permanently.
  TANDEM_UPDATE_SPEC     npm package spec to update from (default: hydrokat/agent-workspace).

Any other arguments are passed to tandem.
USAGE
}

die() {
  local code="$1"
  shift
  printf '%s\n' "$*" >&2
  exit "$code"
}

parse_args() {
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --check) CHECK_ONLY=1 ;;
      --yes) MODE="yes" ;;
      --no) MODE="no" ;;
      --no-update) NO_UPDATE=1 ;;
      --fetch-runtime) FETCH_RUNTIME=1 ;;
      --answers)
        [ "$#" -ge 2 ] || die 4 "--answers needs a value"
        ANSWERS="$2"
        ANSWERS_SET=1
        shift
        ;;
      -h | --help) usage; exit 0 ;;
      --) shift; PASSTHRU+=("$@"); break ;;
      *) PASSTHRU+=("$1") ;;
    esac
    shift
  done
}

load_min_versions() {
  [ -f "$MIN_FILE" ] || die 4 "Missing min-versions.env (run \`npx hydrokat/agent-workspace update\`)"
  local re='^[A-Z_]+=[0-9A-Za-z.-]+$' line n=0
  while IFS= read -r line || [ -n "$line" ]; do
    n=$((n + 1))
    case "$line" in '' | '#'*) continue ;; esac
    [[ $line =~ $re ]] || die 4 "tandem/min-versions.env:$n: invalid line (refusing to continue)"
    MIN_DATA="$MIN_DATA$line"$'\n'
  done <"$MIN_FILE"
}

min_for() {
  local key
  key="TANDEM_MIN_$(printf '%s' "$1" | tr '[:lower:]' '[:upper:]')"
  printf '%s' "$MIN_DATA" | grep -m1 "^$key=" | cut -d= -f2 || true
}

detect_platform() {
  case "$(uname -s)" in
    Darwin)
      PM="brew"
      command -v brew >/dev/null 2>&1 || die 5 "Homebrew is required on macOS. Install it from https://brew.sh, then re-run ./run.sh."
      ;;
    Linux)
      if command -v apt-get >/dev/null 2>&1; then
        PM="apt"
      elif command -v dnf >/dev/null 2>&1; then
        PM="dnf"
      else
        die 5 "Unsupported Linux package manager. Install git, tmux, uv, claude, and agy manually, then re-run."
      fi
      ;;
    *)
      die 5 "Unsupported platform: $(uname -s). Install git, tmux, uv, claude, and agy manually, then re-run."
      ;;
  esac
}

is_ai_tool() {
  case " $AI_TOOLS " in *" $1 "*) return 0 ;; esac
  return 1
}

# Runs a command with a time limit; output lands in CAP_OUT. Returns the command's status.
capture() {
  local secs="$1" tmp pid watcher rc=0
  shift
  tmp="$(mktemp)"
  "$@" >"$tmp" 2>&1 </dev/null &
  pid=$!
  (sleep "$secs"; kill "$pid" 2>/dev/null) >/dev/null 2>&1 &
  watcher=$!
  wait "$pid" 2>/dev/null || rc=$?
  kill "$watcher" 2>/dev/null || true
  wait "$watcher" 2>/dev/null || true
  CAP_OUT="$(cat "$tmp")"
  rm -f "$tmp"
  return "$rc"
}

extract_version() {
  printf '%s\n' "$1" | grep -oE '[0-9]+(\.[0-9]+)*[a-z]?' | head -n 1 || true
}

# True when $1 >= $2. Compares numerically per component; a letter suffix sorts after none (3.3a > 3.3).
version_ge() {
  local a="$1" b="$2" sa="" sb="" i x y max
  case "$a" in *[a-z]) sa="${a: -1}"; a="${a%?}" ;; esac
  case "$b" in *[a-z]) sb="${b: -1}"; b="${b%?}" ;; esac
  local pa pb
  IFS=. read -r -a pa <<<"$a"
  IFS=. read -r -a pb <<<"$b"
  max=${#pa[@]}
  [ "${#pb[@]}" -gt "$max" ] && max=${#pb[@]}
  for ((i = 0; i < max; i++)); do
    x=$((10#${pa[i]:-0}))
    y=$((10#${pb[i]:-0}))
    [ "$x" -gt "$y" ] && return 0
    [ "$x" -lt "$y" ] && return 1
  done
  [[ "$sa" > "$sb" || "$sa" == "$sb" ]]
}

# A Python is healthy when it meets the minimum version and imports hashlib (with blake2) without printing anything:
# builds against a limited OpenSSL log tracebacks on import, which would then appear in every tandem pane.
python_healthy() {
  local min
  min="$(min_for PYTHON)"
  capture 10 "$1" -c 'import hashlib, sys; hashlib.blake2b(); hashlib.blake2s(); hashlib.sha256(); print("%d.%d" % sys.version_info[:2])' || return 1
  [[ "$CAP_OUT" =~ ^[0-9]+\.[0-9]+$ ]] || return 1
  [ -z "$min" ] || version_ge "$CAP_OUT" "$min"
}

select_python() {
  local candidate resolved
  if [ -n "${UV_PYTHON:-}" ] && python_healthy "$UV_PYTHON"; then
    return 0
  fi
  for candidate in python3 python ${TANDEM_PYTHON_FALLBACKS:-/opt/homebrew/bin/python3 /usr/local/bin/python3 /usr/bin/python3}; do
    resolved="$(command -v "$candidate" 2>/dev/null)" || continue
    if python_healthy "$resolved"; then
      export UV_PYTHON="$resolved"
      return 0
    fi
    printf 'Skipping %s: too old or broken hashlib.\n' "$resolved" >&2
  done
  unset UV_PYTHON
  return 1
}

# Only ever deletes a real virtualenv (pyvenv.cfg present) whose interpreter no longer works.
remove_broken_venv() {
  local venv="$1"
  [ -f "$venv/pyvenv.cfg" ] && [ -e "$venv/bin/python" ] || return 0
  python_healthy "$venv/bin/python" && return 0
  printf 'Removing broken virtualenv %s; uv will rebuild it.\n' "$venv" >&2
  rm -rf "$venv"
}

set_var() {
  printf -v "$1" '%s' "$2"
}

get_var() {
  printf '%s' "${!1:-}"
}

detect_tool() {
  local tool="$1" flag="--version"
  [ "$tool" = "tmux" ] && flag="-V"
  hash -r
  if ! command -v "$tool" >/dev/null 2>&1; then
    set_var "INST_$tool" 0
    set_var "VER_$tool" ""
    return
  fi
  set_var "INST_$tool" 1
  local banner=""
  capture 10 "$tool" "$flag" || true
  banner="$CAP_OUT"
  set_var "VER_$tool" "$(extract_version "$banner")"
}

below_min() {
  local tool="$1" ver min
  ver="$(get_var "VER_$tool")"
  min="$(min_for "$tool")"
  [ -n "$ver" ] && [ -n "$min" ] && ! version_ge "$ver" "$min"
}

# Sets LATEST_<tool> to current | available | unknown.
check_latest() {
  local tool="$1" rc=0 state="unknown"
  case "$tool" in
    claude | agy) state="unknown" ;;
    uv)
      capture 15 uv self update --dry-run || rc=$?
      if [ "$rc" -eq 0 ]; then
        if printf '%s' "$CAP_OUT" | grep -q "Would update"; then state="available"; else state="current"; fi
      fi
      ;;
    *)
      case "$PM" in
        brew)
          capture 15 brew outdated --quiet "$tool" || rc=$?
          if [ "$rc" -eq 0 ]; then
            if printf '%s\n' "$CAP_OUT" | grep -qx "$tool"; then state="available"; else state="current"; fi
          fi
          ;;
        apt)
          capture 15 apt-get -s install --only-upgrade "$tool" || rc=$?
          if [ "$rc" -eq 0 ]; then
            if printf '%s\n' "$CAP_OUT" | grep -q "^Inst $tool "; then state="available"; else state="current"; fi
          fi
          ;;
        dnf)
          capture 30 dnf check-update -q "$tool" || rc=$?
          case "$rc" in 100) state="available" ;; 0) state="current" ;; esac
          ;;
      esac
      ;;
  esac
  set_var "LATEST_$tool" "$state"
}

run_installer() {
  curl -fsSL "$1" | bash
}

# Fills CMD with the install command for $1.
install_cmd() {
  case "$1" in
    claude) CMD=(run_installer "$CLAUDE_INSTALL_URL") ;;
    agy) CMD=(run_installer "$AGY_INSTALL_URL") ;;
    uv)
      if [ "$PM" = "brew" ]; then CMD=(brew install uv); else CMD=(run_installer "$UV_INSTALL_URL"); fi
      ;;
    *)
      case "$PM" in
        brew) CMD=(brew install "$1") ;;
        apt) CMD=(sudo apt-get install -y "$1") ;;
        dnf) CMD=(sudo dnf install -y "$1") ;;
      esac
      ;;
  esac
}

update_cmd() {
  case "$1" in
    claude) CMD=(claude update) ;;
    agy) CMD=(agy update) ;;
    uv) CMD=(uv self update) ;;
    *)
      case "$PM" in
        brew) CMD=(brew upgrade "$1") ;;
        apt) CMD=(sudo apt-get install -y --only-upgrade "$1") ;;
        dnf) CMD=(sudo dnf upgrade -y "$1") ;;
      esac
      ;;
  esac
}

describe_cmd() {
  if [ "${CMD[0]}" = "run_installer" ]; then
    printf 'curl -fsSL %s | bash' "${CMD[1]}"
  else
    printf '%q ' "${CMD[@]}" | sed 's/ $//'
  fi
}

execute_cmd() {
  "${CMD[@]}"
}

ask() {
  local reply=""
  printf '%s [y/N] ' "$1"
  if [ "$MODE" = "yes" ]; then
    reply="y"
  elif [ "$MODE" = "no" ]; then
    reply="n"
  elif [ "$ANSWERS_SET" -eq 1 ]; then
    reply="${ANSWERS%%,*}"
    case "$ANSWERS" in *,*) ANSWERS="${ANSWERS#*,}" ;; *) ANSWERS="" ;; esac
  elif [ -t 0 ]; then
    read -r reply || reply=""
  else
    printf '\n'
    die 6 "Input is not a terminal, so I cannot ask. Re-run with --yes, --no, or --check."
  fi
  [ "$MODE" != "" ] || [ "$ANSWERS_SET" -eq 1 ] && printf '%s\n' "$reply"
  case "$(printf '%s' "$reply" | tr '[:upper:]' '[:lower:]')" in y | yes) return 0 ;; esac
  return 1
}

other_ai_installed() {
  local other
  for other in $AI_TOOLS; do
    [ "$other" != "$1" ] && [ "$(get_var "INST_$other")" -eq 1 ] && return 0
  done
  return 1
}

expected_version() {
  local init_file="$TANDEM_DIR/src/tandem/__init__.py"
  if [ -f "$init_file" ]; then
    grep -m1 '__version__' "$init_file" | sed -E 's/.*__version__[[:space:]]*=[[:space:]]*["'"'"']([^"'"'"']+)["'"'"'].*/\1/'
    return 0
  fi
  local manifest="$ROOT_DIR/package.json"
  if [ -f "$manifest" ]; then
    read_manifest_version "$manifest"
    return 0
  fi
  return 1
}

platform_slug() {
  local os arch
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  arch="$(uname -m | tr '[:upper:]' '[:lower:]')"
  case "$os" in
    darwin) os="darwin" ;;
    linux) os="linux" ;;
    *) return 1 ;;
  esac
  case "$arch" in
    x86_64 | amd64) arch="x86_64" ;;
    arm64 | aarch64) arch="arm64" ;;
    *) return 1 ;;
  esac
  printf '%s-%s' "$os" "$arch"
}

binary_dir() {
  local ver="$1" slug="$2"
  printf '%s/bin/%s-%s' "$TANDEM_STATE_DIR" "$ver" "$slug"
}

binary_healthy() {
  local bin="$1" expected="$2"
  [ -x "$bin" ] || return 1
  capture 10 "$bin" --version || return 1
  [ "$CAP_OUT" = "tandem $expected" ]
}

verify_archive_checksum() {
  local file="$1" expected_hash="$2"
  local actual_hash=""
  if command -v shasum >/dev/null 2>&1; then
    actual_hash="$(shasum -a 256 "$file" 2>/dev/null | cut -d' ' -f1)"
  elif command -v sha256sum >/dev/null 2>&1; then
    actual_hash="$(sha256sum "$file" 2>/dev/null | cut -d' ' -f1)"
  else
    return 2
  fi
  [ -n "$actual_hash" ] && [ "$actual_hash" = "$expected_hash" ]
}

fetch_binary() {
  local ver="$1" slug="$2"
  local base="${TANDEM_RELEASE_BASE:-https://github.com/hydrokat/agent-workspace/releases/download}/v${ver}"
  local asset_name="tandem-${ver}-${slug}.tar.gz"
  local staging
  staging="$(mktemp -d)"

  if ! capture 30 curl -fsSL "${base}/SHA256SUMS" -o "$staging/SHA256SUMS"; then
    rm -rf "$staging"
    RUNTIME_REASON="download failed"
    return 1
  fi

  local expected_hash
  expected_hash="$(grep -E "[[:space:]]+${asset_name}$" "$staging/SHA256SUMS" | head -n1 | sed 's/[[:space:]].*//')"
  if [ -z "$expected_hash" ]; then
    rm -rf "$staging"
    RUNTIME_REASON="asset missing from SHA256SUMS"
    return 1
  fi

  if ! capture 60 curl -fsSL "${base}/${asset_name}" -o "$staging/${asset_name}"; then
    rm -rf "$staging"
    RUNTIME_REASON="download failed"
    return 1
  fi

  local check_rc=0
  verify_archive_checksum "$staging/${asset_name}" "$expected_hash" || check_rc=$?
  if [ "$check_rc" -eq 2 ]; then
    rm -rf "$staging"
    RUNTIME_REASON="no sha256 utility found"
    return 1
  elif [ "$check_rc" -ne 0 ]; then
    rm -rf "$staging"
    RUNTIME_REASON="checksum mismatch"
    return 1
  fi

  if ! tar -xzf "$staging/${asset_name}" -C "$staging"; then
    rm -rf "$staging"
    RUNTIME_REASON="archive extraction failed"
    return 1
  fi

  rm -f "$staging/${asset_name}" "$staging/SHA256SUMS"
  touch "$staging/.managed"

  local target_dir
  target_dir="$(binary_dir "$ver" "$slug")"
  mkdir -p "$TANDEM_STATE_DIR/bin"
  rm -rf "$target_dir"
  mv "$staging" "$target_dir"
  return 0
}

prune_binaries() {
  local dir="$TANDEM_STATE_DIR/bin"
  [ -d "$dir" ] || return 0
  local current_target=""
  if [ -L "$dir/current" ]; then
    current_target="$(readlink "$dir/current" 2>/dev/null || true)"
    current_target="${current_target##*/}"
  fi

  local managed_dirs=()
  local d
  for d in "$dir"/*; do
    [ -d "$d" ] || continue
    [ -L "$d" ] && continue
    [ -f "$d/.managed" ] || continue
    managed_dirs+=("$(basename "$d")")
  done

  [ "${#managed_dirs[@]}" -le 2 ] && return 0

  local sorted=()
  while IFS= read -r line; do
    [ -n "$line" ] && sorted+=("$line")
  done < <(printf '%s\n' "${managed_dirs[@]}" | sort -V)

  local total="${#sorted[@]}"
  local to_remove=$((total - 2))
  local i=0
  for ((i = 0; i < to_remove; i++)); do
    local cand="${sorted[i]}"
    if [ "$cand" != "$current_target" ]; then
      rm -rf "$dir/$cand"
    fi
  done
}

select_runtime() {
  local target_runtime="${TANDEM_RUNTIME:-auto}"
  case "$target_runtime" in
    auto | binary | python) ;;
    *) die 4 "Invalid TANDEM_RUNTIME: $target_runtime (must be auto, binary, or python)" ;;
  esac

  if [ "$target_runtime" = "python" ]; then
    RUNTIME="python"
    RUNTIME_REASON=""
    TOOLS="git tmux uv claude agy"
    return 0
  fi

  if ! EXP_VER="$(expected_version)" || [ -z "$EXP_VER" ]; then
    RUNTIME_REASON="unknown version"
    if [ "$target_runtime" = "binary" ]; then
      die 4 "Binary runtime unavailable: $RUNTIME_REASON"
    fi
    RUNTIME="python"
    TOOLS="git tmux uv claude agy"
    return 0
  fi

  if ! SLUG="$(platform_slug)" || [ -z "$SLUG" ]; then
    RUNTIME_REASON="unsupported platform"
    if [ "$target_runtime" = "binary" ]; then
      die 4 "Binary runtime unavailable: $RUNTIME_REASON"
    fi
    RUNTIME="python"
    TOOLS="git tmux uv claude agy"
    return 0
  fi

  local bdir
  bdir="$(binary_dir "$EXP_VER" "$SLUG")"
  local bin="$bdir/tandem/tandem"

  if [ -x "$bin" ] && binary_healthy "$bin" "$EXP_VER"; then
    mkdir -p "$TANDEM_STATE_DIR/bin"
    (cd "$TANDEM_STATE_DIR/bin" && ln -sfn "$EXP_VER-$SLUG" current)
    RUNTIME="binary"
    RUNTIME_REASON=""
    TOOLS="git tmux claude agy"
    return 0
  fi

  if [ "$CHECK_ONLY" -eq 1 ]; then
    RUNTIME="python"
    RUNTIME_REASON="binary not cached"
    TOOLS="git tmux uv claude agy"
    return 0
  fi

  if fetch_binary "$EXP_VER" "$SLUG"; then
    if binary_healthy "$bin" "$EXP_VER"; then
      mkdir -p "$TANDEM_STATE_DIR/bin"
      (cd "$TANDEM_STATE_DIR/bin" && ln -sfn "$EXP_VER-$SLUG" current)
      prune_binaries
      RUNTIME="binary"
      RUNTIME_REASON=""
      TOOLS="git tmux claude agy"
      return 0
    else
      RUNTIME_REASON="smoke test failed"
    fi
  fi

  if [ "$target_runtime" = "binary" ]; then
    die 4 "Binary runtime unavailable: $RUNTIME_REASON"
  fi

  RUNTIME="python"
  TOOLS="git tmux uv claude agy"
  return 0
}

display_ver() {
  local v
  v="$(get_var "VER_$1")"
  if [ -n "$v" ]; then printf '%s' "$v"; else printf 'unknown version'; fi
}

report() {
  local tool min
  printf 'tandem bootstrap · %s (%s)\n' "$(uname -s)" "$PM"
  for tool in $TOOLS; do
    min="$(min_for "$tool")"
    if [ "$(get_var "INST_$tool")" -eq 0 ]; then
      if is_ai_tool "$tool"; then
        if other_ai_installed "$tool"; then
          printf '  ✖ %-7s not installed (optional while another AI CLI is installed)\n' "$tool"
        else
          printf '  ✖ %-7s not installed (at least one AI CLI is required)\n' "$tool"
        fi
      else
        printf '  ✖ %-7s not installed (required)\n' "$tool"
      fi
    elif below_min "$tool"; then
      printf '  ⚠ %-7s %s below minimum %s — may crash\n' "$tool" "$(display_ver "$tool")" "$min"
    else
      case "$(get_var "LATEST_$tool")" in
        available) printf '  ↑ %-7s %s update available\n' "$tool" "$(display_ver "$tool")" ;;
        unknown)
          if is_ai_tool "$tool"; then
            printf '  ✔ %-7s %s (self-updating)\n' "$tool" "$(display_ver "$tool")"
          else
            printf '  ✔ %-7s %s (latest unknown)\n' "$tool" "$(display_ver "$tool")"
          fi
          ;;
        *) printf '  ✔ %-7s %s\n' "$tool" "$(display_ver "$tool")" ;;
      esac
    fi
  done
  if [ "$RUNTIME" = "binary" ]; then
    printf '  runtime: binary %s (%s)\n' "$EXP_VER" "$SLUG"
  elif [ "$CHECK_ONLY" -eq 1 ] && [ "$RUNTIME_REASON" = "binary not cached" ]; then
    printf '  runtime: python (binary not cached)\n'
  else
    printf '  runtime: python\n'
  fi
}

check_status_code() {
  local tool have_ai=0 code=0
  for tool in $TOOLS; do
    if [ "$(get_var "INST_$tool")" -eq 0 ]; then
      is_ai_tool "$tool" || code=2
    else
      is_ai_tool "$tool" && have_ai=1
      below_min "$tool" && [ "$code" -eq 0 ] && code=3
    fi
  done
  [ "$have_ai" -eq 1 ] || code=2
  return "$code"
}

install_tool() {
  local tool="$1"
  install_cmd "$tool"
  if ask "Install $tool with \`$(describe_cmd)\`?"; then
    execute_cmd || die 4 "Installing $tool failed. See the output above."
    detect_tool "$tool"
    [ "$(get_var "INST_$tool")" -eq 1 ] || die 4 "$tool was installed but is not on PATH. Open a new terminal (or add ~/.local/bin to PATH) and re-run."
    return 0
  fi
  return 1
}

update_tool() {
  local tool="$1"
  update_cmd "$tool"
  if ask "Update $tool with \`$(describe_cmd)\`?"; then
    execute_cmd || die 4 "Updating $tool failed. See the output above."
    detect_tool "$tool"
    return 0
  fi
  return 1
}

act() {
  local tool min
  for tool in $TOOLS; do
    min="$(min_for "$tool")"
    if [ "$(get_var "INST_$tool")" -eq 0 ]; then
      if install_tool "$tool"; then continue; fi
      if is_ai_tool "$tool"; then
        printf '  %s skipped (not installed)\n' "$tool"
        continue
      fi
      die 2 "Cannot continue without $tool. Exiting."
    fi
    if below_min "$tool"; then
      printf '%s %s is below the minimum (%s) and may crash.\n' "$tool" "$(display_ver "$tool")" "$min"
      update_tool "$tool" || die 3 "Cannot continue with $tool $(display_ver "$tool"). Exiting."
      below_min "$tool" && die 3 "$tool is still below the minimum ($min) after updating. Exiting."
    elif [ "$(get_var "LATEST_$tool")" = "available" ]; then
      update_tool "$tool" || printf '  continuing with %s %s\n' "$tool" "$(display_ver "$tool")"
    fi
  done
  local have_ai=0
  for tool in $AI_TOOLS; do
    [ "$(get_var "INST_$tool")" -eq 1 ] && have_ai=1
  done
  [ "$have_ai" -eq 1 ] || die 2 "No AI CLI (claude or agy) is installed. Exiting."
}

read_manifest_version() {
  grep -m1 '"version"' "$1" | sed 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/'
}

valid_spec() {
  [[ "$1" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*/[A-Za-z0-9._-]+(#[A-Za-z0-9._/-]+)?$ ]]
}

write_notice() {
  local from_v="$1" to_v="$2"
  local notice="$ROOT_DIR/.agent-workspace/update-notice.env"
  printf 'TANDEM_UPDATE_FROM=%s\nTANDEM_UPDATE_TO=%s\nTANDEM_UPDATE_AT=%s\n' \
    "$from_v" "$to_v" "$(date +%s)" > "$notice.tmp"
  mv "$notice.tmp" "$notice"
}

print_whats_new() {
  local from_v="$1" to_v="$2"
  local changelog="$ROOT_DIR/.agent-workspace/tandem/CHANGELOG.md"
  [ -f "$changelog" ] || return 0

  local bullets="" count=0 collecting=0 line ver bullet
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      '## ['*)
        ver="$(printf '%s\n' "$line" | grep -oE '[0-9]+(\.[0-9]+)*[a-z]?' | head -n1)"
        if [ -n "$ver" ] && ! version_ge "$from_v" "$ver" && version_ge "$to_v" "$ver"; then
          collecting=1
        else
          collecting=0
        fi ;;
      '- '*|'* '*)
        if [ "$collecting" -eq 1 ] && [ "$count" -lt 8 ]; then
          bullet="${line#- }"
          bullet="${bullet#\* }"
          case "$bullet" in
            '**'*'**'*) bullet="${bullet#\*\*}"; bullet="${bullet%%\*\**}" ;;
          esac
          bullet="${bullet%:}"
          bullet="${bullet//\`/}"
          bullets="${bullets}${bullet}"$'\n'
          count=$((count + 1))
        fi ;;
    esac
  done < "$changelog"

  printf '┌ agent-workspace updated %s → %s ──────────────────────────────────┐\n' "$from_v" "$to_v"
  printf '│ %-68s │\n' "What's new"
  while IFS= read -r line || [ -n "$line" ]; do
    [ -n "$line" ] || continue
    [ "${#line}" -le 64 ] || line="${line:0:63}…"
    printf '│  • %-64s │\n' "$line"
  done <<< "$bullets"
  printf '│ %-68s │\n' "Full notes: .agent-workspace/tandem/CHANGELOG.md"
  printf '│ %-68s │\n' "Turn off: ./run.sh --no-update or TANDEM_AUTO_UPDATE=0"
  printf '└──────────────────────────────────────────────────────────────────────┘\n'
}

maybe_update() {
  [ "${TANDEM_SKIP_UPDATE:-0}" = "1" ] && return
  [ "$NO_UPDATE" -eq 1 ] && return
  if [ "${TANDEM_AUTO_UPDATE:-1}" = "0" ]; then
    printf 'Auto-update off (TANDEM_AUTO_UPDATE=0)\n'
    return
  fi

  local manifest="$ROOT_DIR/.agent-workspace/manifest.json"
  [ -f "$manifest" ] || return 0

  command -v npx >/dev/null 2>&1 || { printf 'Skipping update check (npx not found).\n'; return 0; }

  local SPEC="${TANDEM_UPDATE_SPEC:-hydrokat/agent-workspace}"
  if ! valid_spec "$SPEC"; then
    printf '⚠ Invalid TANDEM_UPDATE_SPEC; skipping update.\n'
    return
  fi

  local local_ver rc=0 remote_ver
  local_ver="$(read_manifest_version "$manifest")"
  [ -n "$local_ver" ] || return 0

  printf 'Checking for agent-workspace updates… '
  capture 15 npx --yes "$SPEC" --version || rc=$?
  remote_ver="$(extract_version "$CAP_OUT")"

  if [ "$rc" -ne 0 ] || [ -z "$remote_ver" ]; then
    printf 'Could not check for updates (offline?); continuing with %s\n' "$local_ver"
    return
  fi

  if version_ge "$local_ver" "$remote_ver"; then
    printf 'agent-workspace %s is up to date\n' "$local_ver"
    return
  fi

  printf '%s installed, %s available\n' "$local_ver" "$remote_ver"

  if [ "$CHECK_ONLY" -eq 1 ]; then
    printf '↑ agent-workspace %s → %s available (applied on next launch)\n' "$local_ver" "$remote_ver"
    return
  fi

  printf 'Updating agent-workspace %s → %s …\n' "$local_ver" "$remote_ver"
  rc=0
  npx --yes "$SPEC" update --yes --no-link --no-update-clis --no-commit || rc=$?
  if [ "$rc" -ne 0 ]; then
    printf '⚠ Update failed (exit %s); continuing with %s. Run ./run.sh again or npx %s update\n' \
      "$rc" "$local_ver" "$SPEC"
    return
  fi

  write_notice "$local_ver" "$remote_ver"
  print_whats_new "$local_ver" "$remote_ver"

  export TANDEM_SKIP_UPDATE=1
  exec "$ROOT_DIR/run.sh" ${ORIG_ARGS[@]+"${ORIG_ARGS[@]}"}
}

main() {
  ORIG_ARGS=("$@")
  parse_args "$@"

  if [ "$FETCH_RUNTIME" -eq 1 ]; then
    if ! EXP_VER="$(expected_version)" || [ -z "$EXP_VER" ]; then
      die 4 "Cannot determine expected version"
    fi
    if ! SLUG="$(platform_slug)" || [ -z "$SLUG" ]; then
      die 4 "Cannot determine platform slug"
    fi
    local bdir
    bdir="$(binary_dir "$EXP_VER" "$SLUG")"
    local bin="$bdir/tandem/tandem"
    if ! { [ -x "$bin" ] && binary_healthy "$bin" "$EXP_VER"; }; then
      fetch_binary "$EXP_VER" "$SLUG" || die 4 "Failed to fetch binary: $RUNTIME_REASON"
      binary_healthy "$bin" "$EXP_VER" || die 4 "Binary unhealthy"
    fi
    mkdir -p "$TANDEM_STATE_DIR/bin"
    (cd "$TANDEM_STATE_DIR/bin" && ln -sfn "$EXP_VER-$SLUG" current)
    prune_binaries
    printf 'TANDEM_SELF=%s/bin/current/tandem/tandem\n' "$TANDEM_STATE_DIR"
    exit 0
  fi

  maybe_update
  load_min_versions
  detect_platform
  select_runtime
  local tool
  for tool in $TOOLS; do
    detect_tool "$tool"
    set_var "LATEST_$tool" "unknown"
    [ "$(get_var "INST_$tool")" -eq 1 ] && check_latest "$tool"
  done
  report
  if [ "$CHECK_ONLY" -eq 1 ]; then
    local code=0
    check_status_code || code=$?
    exit "$code"
  fi
  act
  if [ "$RUNTIME" = "binary" ]; then
    export TANDEM_SELF="$TANDEM_STATE_DIR/bin/current/tandem/tandem"
    exec "$TANDEM_SELF" ${PASSTHRU[@]+"${PASSTHRU[@]}"}
  fi
  select_python || true
  remove_broken_venv "$UV_PROJECT_ENVIRONMENT"
  remove_broken_venv "$TANDEM_STATE_DIR/venv"
  uv sync --project "$TANDEM_DIR" --frozen || die 4 "uv sync failed. See the output above."
  exec uv run --project "$TANDEM_DIR" tandem ${PASSTHRU[@]+"${PASSTHRU[@]}"}
}

main "$@"
