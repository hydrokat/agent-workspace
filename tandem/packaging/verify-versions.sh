#!/usr/bin/env bash
# Usage: verify-versions.sh <tag>; fails unless tag == package.json == tandem/Cargo.toml.
set -euo pipefail

die() {
  echo "verify-versions.sh: $*" >&2
  exit 1
}

[ $# -eq 1 ] || die "usage: verify-versions.sh <tag>"

tag="${1#v}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

pkg_json="$repo_root/package.json"
cargo_toml="$repo_root/tandem/Cargo.toml"
[ -f "$pkg_json" ] || die "package.json not found at $pkg_json"
[ -f "$cargo_toml" ] || die "Cargo.toml not found at $cargo_toml"

pkg_version="$(sed -n 's/^[[:space:]]*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$pkg_json" | head -n 1)"
cargo_version="$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$cargo_toml" | head -n 1)"

[ -n "$pkg_version" ] || die "cannot read version from package.json"
[ -n "$cargo_version" ] || die "cannot read version from tandem/Cargo.toml"

[ "$pkg_version" = "$tag" ] || die "package.json version '$pkg_version' != tag version '$tag'"
[ "$cargo_version" = "$tag" ] || die "Cargo.toml version '$cargo_version' != tag version '$tag'"

echo "versions match tag: $tag"
