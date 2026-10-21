#!/bin/sh
name=$(basename "$0")
echo "$name $*" >> "$STATE/calls.log"
case "$name" in
  npm)
    case "$1" in
      view) cat "$STATE/npm.latest" 2>/dev/null || exit 1; exit 0;;
      root) echo "$STATE/npmroot/lib/node_modules"; exit 0;;
      install) [ -f "$STATE/npm.install.fail" ] && { echo "EACCES" >&2; exit 1; }; cp "$STATE/codex.next" "$STATE/codex.version"; exit 0;;
    esac;;
  brew)
    case "$1" in
      outdated) cat "$STATE/brew.outdated" 2>/dev/null; exit 0;;
      upgrade) cp "$STATE/codex.next" "$STATE/codex.version"; exit 0;;
    esac;;
esac
case "$1" in
  --version) cat "$STATE/$name.version" 2>/dev/null || echo "$name 0.0.1"; exit 0;;
  update)
    [ -f "$STATE/$name.update.hang" ] && exec sleep 30
    [ -f "$STATE/$name.update.fail" ] && { echo "update failed for $name" >&2; exit 1; }
    [ -f "$STATE/$name.update.noisy" ] && yes "noise line from $name updater" | head -n 5000
    [ -f "$STATE/$name.next" ] && cp "$STATE/$name.next" "$STATE/$name.version"
    cat >/dev/null
    echo "updated $name"; exit 0;;
esac
exit 0
