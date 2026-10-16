#!/usr/bin/env bash
# tandem bootstrap: check dependencies, offer installs/updates, enforce minimum versions, launch tandem.
#
# Exit codes: 0 ok, 2 declined install, 3 declined/failed required update, 4 install/update/config failure,
#             5 unsupported platform, 6 prompt needed but stdin is not a terminal.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MIN_FILE="$ROOT_DIR/tandem/min-versions.env"
TOOLS="git tmux uv claude agy"
AI_TOOLS="claude agy"
CLAUDE_INSTALL_URL="https://claude.ai/install.sh"
AGY_INSTALL_URL="https://antigravity.google/cli/install.sh"
UV_INSTALL_URL="https://astral.sh/uv/install.sh"

export HOMEBREW_NO_AUTO_UPDATE=1
export HOMEBREW_NO_ENV_HINTS=1
export UV_PROJECT_ENVIRONMENT="${UV_PROJECT_ENVIRONMENT:-${XDG_STATE_HOME:-$HOME/.local/state}/tandem/venv}"

MODE=""
ANSWERS=""
ANSWERS_SET=0
CHECK_ONLY=0
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
  -h, --help       Show this help.

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
  [ -f "$MIN_FILE" ] || die 4 "Missing tandem/min-versions.env"
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

main() {
  parse_args "$@"
  load_min_versions
  detect_platform
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
  uv sync --project "$ROOT_DIR/tandem" --frozen || die 4 "uv sync failed. See the output above."
  exec uv run --project "$ROOT_DIR/tandem" tandem ${PASSTHRU[@]+"${PASSTHRU[@]}"}
}

main "$@"
