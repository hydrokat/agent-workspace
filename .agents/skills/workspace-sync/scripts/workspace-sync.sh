#!/bin/bash

set -euo pipefail

SOURCE_AGENTS_DIR=".agents/agents"
SOURCE_SKILLS_DIR=".agents/skills"

usage() {
  echo "Usage: bash .agents/skills/workspace-sync/scripts/workspace-sync.sh [gemini] [claude] [codex]"
  echo "Provide one or more targets separated by spaces."
}

normalize_targets() {
  local raw=("$@")
  local normalized=()
  local item

  for item in "${raw[@]}"; do
    item="${item//,/ }"
    for token in $item; do
      case "$token" in
        gemini|claude|codex)
          normalized+=("$token")
          ;;
        *)
          echo "Error: Unsupported target '$token'." >&2
          usage
          exit 1
          ;;
      esac
    done
  done

  printf "%s\n" "${normalized[@]}" | awk '!seen[$0]++'
}

ensure_source_dir() {
  local source_dir="$1"
  if [ ! -d "$source_dir" ]; then
    echo "Error: $source_dir not found." >&2
    exit 1
  fi
}

ensure_agents_md() {
  if [ ! -f "AGENTS.md" ]; then
    echo "Error: AGENTS.md not found." >&2
    exit 1
  fi
}

sync_docs_link() {
  local target_doc="$1"
  local current_target=""

  if [ -L "$target_doc" ]; then
    current_target="$(readlink "$target_doc")"
    if [ "$current_target" = "AGENTS.md" ] || [ "$current_target" = "./AGENTS.md" ]; then
      echo "Preserving existing managed link: $target_doc -> $current_target"
      return 0
    fi

    echo "Skipping $target_doc to avoid overwriting an existing unmanaged symlink."
    return 0
  fi

  if [ -e "$target_doc" ]; then
    echo "Skipping $target_doc to avoid overwriting an existing file."
    return 0
  fi

  echo "Creating $target_doc -> AGENTS.md"
  ln -s AGENTS.md "$target_doc"
}

clear_target_dir() {
  local target_dir="$1"
  mkdir -p "$target_dir"
  echo "Clearing generated agents in $target_dir..."
  rm -f "$target_dir"/*.md
}

clear_target_tree() {
  local target_dir="$1"
  mkdir -p "$target_dir"
  echo "Clearing generated content in $target_dir..."
  find "$target_dir" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
}

sync_skill_tree() {
  local source_dir="$1"
  local target_dir="$2"
  local dir skill_name target_path

  clear_target_tree "$target_dir"
  echo "Copying skills from $source_dir to $target_dir..."

  for dir in "$source_dir"/*/; do
    [ -d "$dir" ] || continue

    skill_name=$(basename "$dir")
    target_path="$target_dir/$skill_name"
    cp -R "$dir" "$target_path"
    echo "Synced skill: $skill_name"
  done
}

sync_gemini_agents() {
  local target_dir=".gemini/agents"
  local dir agent_name agent_file target_file

  sync_docs_link "GEMINI.md"
  clear_target_dir "$target_dir"

  echo "Transforming and copying agents to $target_dir for Gemini compatibility..."

  for dir in "$SOURCE_AGENTS_DIR"/*/; do
    [ -d "$dir" ] || continue

    agent_name=$(basename "$dir")
    agent_file="$dir/AGENT.md"
    target_file="$target_dir/$agent_name.md"

    if [ -f "$agent_file" ]; then
      sed -E 's/^tools:.*$/tools: ["*"]/' "$agent_file" > "$target_file"
      echo "Synced Gemini agent: $agent_name"
    else
      echo "Skipped Gemini agent: $agent_name (no AGENT.md found)"
    fi
  done
}

sync_gemini_skills() {
  sync_skill_tree "$SOURCE_SKILLS_DIR" ".gemini/skills"
}

sync_claude_agents() {
  local target_dir=".claude/agents"
  local dir agent_name agent_file target_file

  sync_docs_link "CLAUDE.md"
  clear_target_dir "$target_dir"

  echo "Transforming and copying agents to $target_dir for Claude compatibility..."

  for dir in "$SOURCE_AGENTS_DIR"/*/; do
    [ -d "$dir" ] || continue

    agent_name=$(basename "$dir")
    agent_file="$dir/AGENT.md"
    target_file="$target_dir/$agent_name.md"

    if [ -f "$agent_file" ]; then
      sed '/^tools:/d' "$agent_file" > "$target_file"
      echo "Synced Claude agent: $agent_name"
    else
      echo "Skipped Claude agent: $agent_name (no AGENT.md found)"
    fi
  done
}

sync_claude_skills() {
  sync_skill_tree "$SOURCE_SKILLS_DIR" ".claude/skills"
}

sync_codex_workspace() {
  echo "Codex compatibility uses the native workspace layout."
  echo "Verified: AGENTS.md is the canonical instructions file."
  echo "Verified: $SOURCE_AGENTS_DIR remains the canonical agent directory."
  echo "Verified: $SOURCE_SKILLS_DIR remains the canonical skill directory."
}

main() {
  local -a targets
  local target

  if [ "$#" -eq 0 ]; then
    usage
    exit 1
  fi

  ensure_source_dir "$SOURCE_AGENTS_DIR"
  ensure_source_dir "$SOURCE_SKILLS_DIR"
  ensure_agents_md

  while IFS= read -r target; do
    [ -n "$target" ] && targets+=("$target")
  done < <(normalize_targets "$@")

  for target in "${targets[@]}"; do
    case "$target" in
      gemini)
        sync_gemini_agents
        sync_gemini_skills
        ;;
      claude)
        sync_claude_agents
        sync_claude_skills
        ;;
      codex)
        sync_codex_workspace
        ;;
    esac
  done

  echo "Sync complete for targets: ${targets[*]}"
}

main "$@"
