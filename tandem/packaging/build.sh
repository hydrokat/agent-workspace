#!/usr/bin/env bash
# Build a tandem release archive: build.sh [--target <triple>] [--expect-version <ver>]
# Prints the archive path on success. Writes <asset> and <asset>.sha256 to <crate>/dist.
set -euo pipefail

SUPPORTED_TARGETS="aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-pc-windows-msvc"

die() {
  echo "build.sh: $*" >&2
  exit 1
}

slug_for_triple() {
  case "$1" in
    aarch64-apple-darwin) printf 'darwin-arm64' ;;
    x86_64-apple-darwin) printf 'darwin-x86_64' ;;
    x86_64-unknown-linux-musl) printf 'linux-x86_64' ;;
    aarch64-unknown-linux-musl) printf 'linux-arm64' ;;
    x86_64-pc-windows-msvc) printf 'windows-x86_64' ;;
    *) return 1 ;;
  esac
}

asset_name() {
  local version="$1" slug="$2"
  case "$slug" in
    windows-*) printf 'tandem-%s-%s.zip' "$version" "$slug" ;;
    *) printf 'tandem-%s-%s.tar.gz' "$version" "$slug" ;;
  esac
}

exe_name() {
  case "$1" in
    windows-*) printf 'tandem.exe' ;;
    *) printf 'tandem' ;;
  esac
}

host_slug() {
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

default_triple() {
  case "$(host_slug)" in
    darwin-arm64) printf 'aarch64-apple-darwin' ;;
    darwin-x86_64) printf 'x86_64-apple-darwin' ;;
    linux-x86_64) printf 'x86_64-unknown-linux-musl' ;;
    linux-arm64) printf 'aarch64-unknown-linux-musl' ;;
    windows-x86_64) printf 'x86_64-pc-windows-msvc' ;;
    *) return 1 ;;
  esac
}

cargo_version() {
  sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$1/Cargo.toml" | head -n 1
}

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    die "need shasum or sha256sum"
  fi
}

write_sha256() {
  local archive="$1"
  printf '%s  %s\n' "$(sha256_of "$archive")" "$(basename "$archive")" >"$archive.sha256"
}

create_archive() {
  local dist="$1" archive="$2" slug="$3"
  rm -f "$archive"
  case "$slug" in
    windows-*)
      if command -v zip >/dev/null 2>&1; then
        (cd "$dist" && zip -q -r "$archive" tandem)
      elif command -v 7z >/dev/null 2>&1; then
        (cd "$dist" && 7z a -tzip -bso0 "$archive" tandem)
      else
        die "need zip or 7z to build the windows archive"
      fi
      ;;
    *)
      tar -czf "$archive" -C "$dist" tandem
      ;;
  esac
}

smoke_test() {
  local bin="$1" version="$2" out
  out="$(env -i PATH=/usr/bin:/bin "$bin" --version)" || die "built binary failed --version"
  case "$out" in
    *"tandem $version"*) ;;
    *) die "unexpected version output: $out" ;;
  esac
  env -i PATH=/usr/bin:/bin "$bin" --help >/dev/null || die "built binary failed --help"
}

main() {
  local target="" expect_version=""
  while [ $# -gt 0 ]; do
    case "$1" in
      --target) target="${2:-}"; shift 2 ;;
      --expect-version) expect_version="${2:-}"; shift 2 ;;
      *) die "unknown argument: $1" ;;
    esac
  done

  local script_dir crate_root version
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  crate_root="$(cd "$script_dir/.." && pwd)"
  version="$(cargo_version "$crate_root")"
  [ -n "$version" ] || die "cannot read version from $crate_root/Cargo.toml"

  if [ -n "$expect_version" ] && [ "$expect_version" != "$version" ]; then
    die "version mismatch: expected $expect_version but Cargo.toml has $version"
  fi

  if [ -z "$target" ]; then
    target="$(default_triple)" || die "cannot infer a supported target for this host; pass --target"
  fi
  local slug
  slug="$(slug_for_triple "$target")" || die "unsupported target: $target (supported: $SUPPORTED_TARGETS)"

  case "$target" in
    *-apple-darwin) export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C link-arg=-mmacosx-version-min=13.0" ;;
  esac

  (cd "$crate_root" && cargo build --release --locked --target "$target") || die "cargo build failed for $target"

  local exe built target_dir dist stage archive
  exe="$(exe_name "$slug")"
  target_dir="${CARGO_TARGET_DIR:-$crate_root/target}"
  built="$target_dir/$target/release/$exe"
  [ -f "$built" ] || die "built binary not found: $built"

  dist="$crate_root/dist"
  stage="$dist/tandem"
  rm -rf "$stage"
  mkdir -p "$stage"
  cp "$built" "$stage/$exe"
  chmod +x "$stage/$exe"

  if [ "$slug" = "$(host_slug 2>/dev/null || true)" ]; then
    smoke_test "$stage/$exe" "$version"
  fi

  archive="$dist/$(asset_name "$version" "$slug")"
  create_archive "$dist" "$archive" "$slug"
  write_sha256 "$archive"
  printf '%s\n' "$archive"
}

if [ -z "${BUILD_SH_SOURCE_ONLY:-}" ]; then
  main "$@"
fi
