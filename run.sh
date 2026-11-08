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
TOOLS="git tmux claude agy"
AI_TOOLS="claude agy"
CLAUDE_INSTALL_URL="https://claude.ai/install.sh"
AGY_INSTALL_URL="https://antigravity.google/cli/install.sh"
RUSTUP_INSTALL_URL="https://sh.rustup.rs"

export HOMEBREW_NO_AUTO_UPDATE=1
export HOMEBREW_NO_ENV_HINTS=1
TANDEM_STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/tandem"
BUILD_SLUG="$(printf '%s' "$TANDEM_DIR" | tr -c 'A-Za-z0-9' '-')"
BUILD_SLUG="${BUILD_SLUG#-}"

add_cargo_bin_to_path() {
  case ":$PATH:" in *":$HOME/.cargo/bin:"*) return 0 ;; esac
  [ ! -d "$HOME/.cargo/bin" ] || PATH="$HOME/.cargo/bin:$PATH"
}
add_cargo_bin_to_path

FETCH_RUNTIME=0
RUNTIME=""
RUNTIME_REASON=""
SOURCE_EXPLICIT=0
SOURCE_REUSE=0
EXP_VER=""
SLUG=""
MODE=""
FRESH=0
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

Checks git, tmux, claude, and agy (plus cargo when tandem has to be built from source),
asks before installing or updating anything, then launches tandem from the current directory.

Options:
  --check          Report status only; change nothing, launch nothing.
  --yes            Answer yes to every prompt.
  --no             Answer no to every prompt.
  --fresh          Clear coordination directory and start tandem fresh.
  --answers y,n,.. Answer prompts in order from a list (automation); missing answers mean no.
  --no-update      Skip the launch-time workspace self-update check.
  -h, --help       Show this help.

Environment:
  TANDEM_RUNTIME         auto (default), binary, or source (build with cargo).
                         In auto mode a build on this machine only happens after you say yes.
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
      --fresh) FRESH=1 ;;
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
        die 5 "Unsupported Linux package manager. Install git, tmux, claude, and agy manually, then re-run."
      fi
      ;;
    MINGW* | MSYS* | CYGWIN*)
      if command -v pacman >/dev/null 2>&1; then
        PM="pacman"
      else
        die 5 "Install MSYS2 and run from its shell, or use WSL."
      fi
      ;;
    *)
      die 5 "Unsupported platform: $(uname -s). Install git, tmux, claude, and agy manually, then re-run."
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
  add_cargo_bin_to_path
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
    cargo) state="unknown" ;;
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
        pacman)
          capture 15 pacman -Qu "$tool" || rc=$?
          if [ "$rc" -eq 0 ] && [ -n "$CAP_OUT" ]; then
            if printf '%s\n' "$CAP_OUT" | grep -q "^$tool "; then state="available"; else state="current"; fi
          fi
          ;;
      esac
      ;;
  esac
  set_var "LATEST_$tool" "$state"
}

run_installer() {
  curl -fsSL "$1" | bash
}

run_rustup_installer() {
  curl --proto '=https' --tlsv1.2 -sSf "$RUSTUP_INSTALL_URL" | sh -s -- -y --profile minimal
}

# Fills CMD with the install command for $1.
install_cmd() {
  case "$1" in
    claude) CMD=(run_installer "$CLAUDE_INSTALL_URL") ;;
    agy) CMD=(run_installer "$AGY_INSTALL_URL") ;;
    cargo) CMD=(run_rustup_installer) ;;
    *)
      case "$PM" in
        brew) CMD=(brew install "$1") ;;
        apt) CMD=(sudo apt-get install -y "$1") ;;
        dnf) CMD=(sudo dnf install -y "$1") ;;
        pacman) CMD=(pacman -S --noconfirm "$1") ;;
      esac
      ;;
  esac
}

update_cmd() {
  case "$1" in
    claude) CMD=(claude update) ;;
    agy) CMD=(agy update) ;;
    cargo) CMD=(rustup update) ;;
    *)
      case "$PM" in
        brew) CMD=(brew upgrade "$1") ;;
        apt) CMD=(sudo apt-get install -y --only-upgrade "$1") ;;
        dnf) CMD=(sudo dnf upgrade -y "$1") ;;
        pacman) CMD=(pacman -Syu --noconfirm "$1") ;;
      esac
      ;;
  esac
}

describe_cmd() {
  if [ "${CMD[0]}" = "run_installer" ]; then
    printf 'curl -fsSL %s | bash' "${CMD[1]}"
  elif [ "${CMD[0]}" = "run_rustup_installer" ]; then
    printf "curl --proto '=https' --tlsv1.2 -sSf %s | sh -s -- -y --profile minimal" "$RUSTUP_INSTALL_URL"
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

cargo_manifest_version() {
  sed -n '/^\[package\]/,/^\[/{s/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p;}' "$1" | head -n 1
}

expected_version() {
  local cargo_toml="$TANDEM_DIR/Cargo.toml" ver=""
  if [ -f "$cargo_toml" ]; then
    ver="$(cargo_manifest_version "$cargo_toml")"
    if [ -n "$ver" ]; then
      printf '%s\n' "$ver"
      return 0
    fi
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
    mingw* | msys* | cygwin*) os="windows" ;;
    *) return 1 ;;
  esac
  case "$arch" in
    x86_64 | amd64) arch="x86_64" ;;
    arm64 | aarch64) arch="arm64" ;;
    *) return 1 ;;
  esac
  printf '%s-%s' "$os" "$arch"
}

binary_exe_name() {
  local slug="$1"
  case "$slug" in
    windows-*) printf 'tandem.exe' ;;
    *) printf 'tandem' ;;
  esac
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
  local archive_ext="tar.gz"
  case "$slug" in
    windows-*) archive_ext="zip" ;;
  esac
  local asset_name="tandem-${ver}-${slug}.${archive_ext}"
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

  if [ "$archive_ext" = "zip" ]; then
    if ! command -v unzip >/dev/null 2>&1; then
      rm -rf "$staging"
      RUNTIME_REASON="unzip utility missing"
      return 1
    fi
    if ! unzip -q "$staging/${asset_name}" -d "$staging"; then
      rm -rf "$staging"
      RUNTIME_REASON="archive extraction failed"
      return 1
    fi
  else
    if ! tar -xzf "$staging/${asset_name}" -C "$staging"; then
      rm -rf "$staging"
      RUNTIME_REASON="archive extraction failed"
      return 1
    fi
  fi

  rm -f "$staging/${asset_name}" "$staging/SHA256SUMS"
  : > "$staging/.managed"

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

source_build_dir() {
  printf '%s/build-%s' "$TANDEM_STATE_DIR" "$BUILD_SLUG"
}

use_source_runtime() {
  RUNTIME="source"
  TOOLS="git tmux cargo claude agy"
  if [ "$SOURCE_EXPLICIT" -eq 0 ] && [ -n "$EXP_VER" ]; then
    local bin
    bin="$(source_build_dir)/release/$(source_exe_name)"
    if [ -x "$bin" ] && binary_healthy "$bin" "$EXP_VER"; then
      SOURCE_REUSE=1
      TOOLS="git tmux claude agy"
    fi
  fi
}

use_binary_runtime() {
  mkdir -p "$TANDEM_STATE_DIR/bin"
  (cd "$TANDEM_STATE_DIR/bin" && ln -sfn "$EXP_VER-$SLUG" current)
  RUNTIME="binary"
  RUNTIME_REASON=""
  TOOLS="git tmux claude agy"
}

select_runtime() {
  local target_runtime="${TANDEM_RUNTIME:-auto}"
  case "$target_runtime" in
    auto | binary | source) ;;
    *) die 4 "Invalid TANDEM_RUNTIME: $target_runtime (must be auto, binary, or source)" ;;
  esac

  if [ "$target_runtime" = "source" ]; then
    SOURCE_EXPLICIT=1
    EXP_VER="$(expected_version || true)"
    use_source_runtime
    return 0
  fi

  if ! EXP_VER="$(expected_version)" || [ -z "$EXP_VER" ]; then
    RUNTIME_REASON="unknown version"
    if [ "$target_runtime" = "binary" ]; then
      die 4 "Binary runtime unavailable: $RUNTIME_REASON"
    fi
    use_source_runtime
    return 0
  fi

  if ! SLUG="$(platform_slug)" || [ -z "$SLUG" ]; then
    RUNTIME_REASON="no prebuilt binary for this platform"
    if [ "$target_runtime" = "binary" ]; then
      die 4 "Binary runtime unavailable: $RUNTIME_REASON"
    fi
    use_source_runtime
    return 0
  fi

  local bdir exe_name bin
  bdir="$(binary_dir "$EXP_VER" "$SLUG")"
  exe_name="$(binary_exe_name "$SLUG")"
  bin="$bdir/tandem/$exe_name"

  if [ -x "$bin" ] && binary_healthy "$bin" "$EXP_VER"; then
    use_binary_runtime
    return 0
  fi

  if [ "$CHECK_ONLY" -eq 1 ]; then
    RUNTIME_REASON="binary not cached"
    use_source_runtime
    return 0
  fi

  if fetch_binary "$EXP_VER" "$SLUG"; then
    if binary_healthy "$bin" "$EXP_VER"; then
      use_binary_runtime
      prune_binaries
      return 0
    fi
    RUNTIME_REASON="smoke test failed"
  fi

  if [ "$target_runtime" = "binary" ]; then
    die 4 "Binary runtime unavailable: $RUNTIME_REASON"
  fi

  use_source_runtime
  return 0
}

display_ver() {
  local v
  v="$(get_var "VER_$1")"
  if [ -n "$v" ]; then printf '%s' "$v"; else printf 'unknown version'; fi
}

cargo_optional_for_now() {
  [ "$CHECK_ONLY" -eq 1 ] && [ "$RUNTIME_REASON" = "binary not cached" ]
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
      elif [ "$tool" = "cargo" ] && cargo_optional_for_now; then
        printf '  ✖ %-7s not installed (only needed if the binary download fails)\n' "$tool"
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
    printf '  runtime: source (binary not cached)\n'
  else
    printf '  runtime: source\n'
  fi
}

check_status_code() {
  local tool have_ai=0 code=0
  for tool in $TOOLS; do
    if [ "$(get_var "INST_$tool")" -eq 0 ]; then
      if [ "$tool" = "cargo" ] && cargo_optional_for_now; then continue; fi
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

remove_retired_runtime_state() {
  local venv
  for venv in "$TANDEM_STATE_DIR"/venv "$TANDEM_STATE_DIR"/venv-*; do
    [ -f "$venv/pyvenv.cfg" ] && rm -rf "$venv"
  done
  return 0
}

resolve_coord_dir() {
  if [ -n "${TANDEM_COORD_DIR:-}" ]; then
    printf '%s\n' "$TANDEM_COORD_DIR"
  else
    printf '%s/.agent-workspace/coordination\n' "$ROOT_DIR"
  fi
}

collect_coord_files() {
  local target="$1" entry
  for entry in "$target"/* "$target"/.*; do
    case "$entry" in
      "$target"/\* | "$target"/'.' | "$target"/'..' | "$target"/.gitignore) continue ;;
    esac
    if [ -d "$entry" ] && ! [ -L "$entry" ]; then
      collect_coord_files "$entry"
    elif [ -f "$entry" ] || [ -L "$entry" ]; then
      COORD_FILES+=("$entry")
    fi
  done
}

is_tandem_running() {
  if tmux has-session -t tandem 2>/dev/null; then
    return 0
  fi
  local sock_dir="${TMUX_TMPDIR:-${TMPDIR:-/tmp}}/tmux-$(id -u 2>/dev/null || true)"
  if [ -d "$sock_dir" ]; then
    local sock sname
    for sock in "$sock_dir"/tandem* "$sock_dir"/*; do
      if [ -S "$sock" ]; then
        sname="$(basename "$sock")"
        if tmux -L "$sname" has-session -t tandem 2>/dev/null; then
          local ws_env pane_path
          ws_env="$(tmux -L "$sname" show-environment -g -t tandem TANDEM_WORKSPACE 2>/dev/null | cut -d= -f2- || true)"
          [ -n "$ws_env" ] && [ "$ws_env" = "$ROOT_DIR" ] && return 0
          pane_path="$(tmux -L "$sname" display-message -p -t tandem '#{pane_current_path}' 2>/dev/null || true)"
          [ -n "$pane_path" ] && [ "$pane_path" = "$ROOT_DIR" ] && return 0
        fi
      fi
    done
  fi
  return 1
}

add_fresh_passthru() {
  local has_fresh=0 a
  for a in ${PASSTHRU[@]+"${PASSTHRU[@]}"}; do
    [ "$a" = "--fresh" ] && has_fresh=1
  done
  if [ "$has_fresh" -eq 0 ]; then
    PASSTHRU=("--fresh" ${PASSTHRU[@]+"${PASSTHRU[@]}"})
  fi
}

fresh_reset() {
  [ "$FRESH" -eq 1 ] || return 0
  local coord_dir
  coord_dir="$(resolve_coord_dir)"

  [ -L "$coord_dir" ] && die 4 "Refusing to operate on symlinked coordination directory: $coord_dir"
  [ "$coord_dir" != "/" ] || die 4 "Unsafe coordination directory: $coord_dir"
  [ -z "${HOME:-}" ] || [ "$coord_dir" != "$HOME" ] || die 4 "Unsafe coordination directory: $coord_dir"
  [ "$coord_dir" != "$ROOT_DIR" ] || die 4 "Unsafe coordination directory: $coord_dir"

  if [ -n "${TANDEM_COORD_DIR:-}" ]; then
    [ "$coord_dir" = "$TANDEM_COORD_DIR" ] || die 4 "Coordination directory mismatch: $coord_dir"
  else
    case "$coord_dir" in
      "$ROOT_DIR"/*) ;;
      *) die 4 "Coordination directory is outside workspace: $coord_dir" ;;
    esac
  fi

  local rel_coord="$coord_dir"
  case "$coord_dir" in
    "$ROOT_DIR"/*) rel_coord="${coord_dir#"$ROOT_DIR"/}" ;;
  esac

  if ! [ -e "$coord_dir" ]; then
    if [ "$CHECK_ONLY" -eq 1 ]; then
      printf 'Nothing to clear in %s. (No changes made.)\n' "$rel_coord"
      return 0
    fi
    mkdir -m 0700 -p "$coord_dir"
    local sub
    for sub in claims conversations queries sessions; do
      mkdir -m 0700 -p "$coord_dir/$sub"
    done
    printf 'Nothing to clear in %s.\n' "$rel_coord"
    add_fresh_passthru
    return 0
  fi

  [ -d "$coord_dir" ] || die 4 "Coordination path is not a directory: $coord_dir"

  COORD_FILES=()
  collect_coord_files "$coord_dir"
  local count="${#COORD_FILES[@]}"

  if [ "$CHECK_ONLY" -eq 1 ]; then
    if [ "$count" -eq 0 ]; then
      printf 'Nothing to clear in %s. (No changes made.)\n' "$rel_coord"
    else
      printf 'Would delete %s files in %s. (No changes made.)\n' "$count" "$rel_coord"
    fi
    return 0
  fi

  if is_tandem_running; then
    local opt_key="Ctrl+Alt+x"
    [ "$(uname -s)" = "Darwin" ] && opt_key="Ctrl+Opt+x"
    die 2 "tandem is running for this workspace. Quit it first ($opt_key → Save and exit), then run ./run.sh --fresh."
  fi

  if [ "$count" -eq 0 ]; then
    printf 'Nothing to clear in %s.\n' "$rel_coord"
    add_fresh_passthru
    return 0
  fi

  ask "Fresh start: this deletes $count files in $rel_coord
(sessions, claims, queries, conversations). Agents' saved session is replaced.
Continue?" || die 2 "Declined clearing coordination directory."

  local f
  for f in "${COORD_FILES[@]}"; do
    rm -f "$f"
  done

  local s
  for s in claims conversations queries sessions; do
    mkdir -m 0700 -p "$coord_dir/$s"
    chmod 0700 "$coord_dir/$s" 2>/dev/null || true
  done
  chmod 0700 "$coord_dir" 2>/dev/null || true

  printf 'Cleared %s files. Starting tandem fresh…\n' "$count"
  add_fresh_passthru
}

source_exe_name() {
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*) printf 'tandem.exe' ;;
    *) printf 'tandem' ;;
  esac
}

confirm_source_build() {
  [ "$RUNTIME" = "source" ] || return 0
  [ "$SOURCE_EXPLICIT" -eq 0 ] || return 0
  [ "$SOURCE_REUSE" -eq 0 ] || return 0
  local why="${RUNTIME_REASON:-no usable prebuilt binary}"
  ask "Build tandem from source? No usable prebuilt binary ($why). This compiles it on this machine with \`cargo build --release --locked\` into $(source_build_dir) and can take a few minutes." \
    || die 2 "Declined building tandem on this machine. Re-run ./run.sh and answer y (or pass --yes), or install a release binary for your platform."
}

run_source() {
  [ -f "$TANDEM_DIR/Cargo.toml" ] || die 4 "tandem sources not found at $TANDEM_DIR/Cargo.toml (run \`npx hydrokat/agent-workspace update\`)"
  local target_dir bin
  target_dir="$(source_build_dir)"
  if [ "$SOURCE_REUSE" -eq 0 ]; then
    printf 'Building tandem from source with cargo (first build takes a few minutes)…\n'
    cargo build --release --locked --manifest-path "$TANDEM_DIR/Cargo.toml" --target-dir "$target_dir" \
      || die 4 "cargo build failed. See the output above."
  fi
  bin="$target_dir/release/$(source_exe_name)"
  [ -x "$bin" ] || die 4 "cargo build produced no binary at $bin"
  capture 10 "$bin" --version || die 4 "The freshly built tandem failed its --version smoke test."
  case "$CAP_OUT" in
    "tandem ${EXP_VER:-}"*) ;;
    *) die 4 "Unexpected --version output from the built tandem: $CAP_OUT" ;;
  esac
  export TANDEM_SELF="$bin"
  exec "$bin" ${PASSTHRU[@]+"${PASSTHRU[@]}"}
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
    local bdir exe_name bin
    bdir="$(binary_dir "$EXP_VER" "$SLUG")"
    exe_name="$(binary_exe_name "$SLUG")"
    bin="$bdir/tandem/$exe_name"
    if ! { [ -x "$bin" ] && binary_healthy "$bin" "$EXP_VER"; }; then
      fetch_binary "$EXP_VER" "$SLUG" || die 4 "Failed to fetch binary: $RUNTIME_REASON"
      binary_healthy "$bin" "$EXP_VER" || die 4 "Binary unhealthy"
    fi
    mkdir -p "$TANDEM_STATE_DIR/bin"
    (cd "$TANDEM_STATE_DIR/bin" && ln -sfn "$EXP_VER-$SLUG" current)
    prune_binaries
    printf 'TANDEM_SELF=%s/bin/current/tandem/%s\n' "$TANDEM_STATE_DIR" "$exe_name"
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
    fresh_reset
    local code=0
    check_status_code || code=$?
    exit "$code"
  fi
  confirm_source_build
  act
  remove_retired_runtime_state
  fresh_reset
  if [ "$RUNTIME" = "binary" ]; then
    export TANDEM_SELF="$TANDEM_STATE_DIR/bin/current/tandem/$(binary_exe_name "$SLUG")"
    exec "$TANDEM_SELF" ${PASSTHRU[@]+"${PASSTHRU[@]}"}
  fi
  run_source
}

main "$@"
