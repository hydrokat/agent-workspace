#!/bin/bash

set -euo pipefail

TARGET_DIR="${1:-.}"

ensure_dir() {
  local path="$1"
  mkdir -p "$path"
  echo "Ensured directory: $path"
}

create_file_if_missing() {
  local path="$1"

  if [ -e "$path" ]; then
    echo "Preserved existing file: $path"
    return 0
  fi

  mkdir -p "$(dirname "$path")"
  cat > "$path"
  echo "Created file: $path"
}

init_directories() {
  ensure_dir "$TARGET_DIR/.agents/agents"
  ensure_dir "$TARGET_DIR/.agents/skills"
  ensure_dir "$TARGET_DIR/knowledgebase/guidelines"
  ensure_dir "$TARGET_DIR/knowledgebase/best-practices"
  ensure_dir "$TARGET_DIR/knowledgebase/business-flows"
  ensure_dir "$TARGET_DIR/knowledgebase/context-history"
  ensure_dir "$TARGET_DIR/specs"
}

init_agents_md() {
  create_file_if_missing "$TARGET_DIR/AGENTS.md" <<'EOF'
# AGENTS.md - Project Workspace

This file defines the base operating rules for this workspace.

## Core Directory Map
- `/.agents/agents`: Agent definitions
- `/.agents/skills`: Workspace-local skills
- `/knowledgebase`: Project knowledge and standards
- `/specs`: Planning and task tracking

## Operating Principles
- Read first before making structural changes.
- Update base files, then sync compatibility outputs if needed.
- Keep changes small and documented.
EOF
}

init_workflow_md() {
  create_file_if_missing "$TARGET_DIR/WORKFLOW.md" <<'EOF'
# WORKFLOW.md

## Default Workflow
- Read before changing.
- Prefer simple, low-risk updates.
- Validate after changes.
- Keep documentation aligned with implementation.
EOF
}

init_knowledgebase_files() {
  create_file_if_missing "$TARGET_DIR/knowledgebase/README.md" <<'EOF'
# Shared Knowledgebase

This directory stores project context, standards, business flows, and historical decisions.
EOF

  create_file_if_missing "$TARGET_DIR/knowledgebase/guidelines/README.md" <<'EOF'
# Guidelines

Use this directory for stable policies, operating standards, and rules.
EOF

  create_file_if_missing "$TARGET_DIR/knowledgebase/best-practices/README.md" <<'EOF'
# Best Practices

Use this directory for reusable patterns, lessons learned, and recommended approaches.
EOF

  create_file_if_missing "$TARGET_DIR/knowledgebase/business-flows/README.md" <<'EOF'
# Business Flows

Use this directory for process flows, decision paths, and operational context.
EOF

  create_file_if_missing "$TARGET_DIR/knowledgebase/context-history/README.md" <<'EOF'
# Context History

Use this directory for important decisions, historical context, and major changes.
EOF
}

init_specs_files() {
  create_file_if_missing "$TARGET_DIR/specs/README.md" <<'EOF'
# Specs

Use this directory for implementation plans, tasks, and execution breakdowns.
EOF
}

main() {
  init_directories
  init_agents_md
  init_workflow_md
  init_knowledgebase_files
  init_specs_files
  echo "Workspace initialization complete: $TARGET_DIR"
}

main "$@"
