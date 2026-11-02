import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/explain/scripts/locate.py"


def git(repo: Path, *args: str) -> None:
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True,
                   env={**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null"})


def make_repo(path: Path, files: dict[str, str | bytes]) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    git(path, "init", "-q")
    for name, content in files.items():
        target = path / name
        target.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, bytes):
            target.write_bytes(content)
        else:
            target.write_text(content)
    git(path, "add", "-f", "-A")
    return path


def run(cwd: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True, cwd=cwd)


def out(proc: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    assert proc.returncode == 0, proc.stdout + proc.stderr
    data: dict[str, Any] = json.loads(proc.stdout)
    return data


def err(proc: subprocess.CompletedProcess[str], code: int) -> dict[str, Any]:
    assert proc.returncode == code, proc.stdout + proc.stderr
    data: dict[str, Any] = json.loads(proc.stdout)
    assert "error" in data
    return data


def paths(data: dict[str, Any], key: str = "files") -> list[str]:
    return [f"{item['repo']}:{item['path']}" for item in data[key]]


@pytest.fixture
def workspace(tmp_path: Path) -> Path:
    ws = make_repo(tmp_path / "ws", {
        "README.md": "# ws\nmentions sync once\n",
        "src/core/sync.js": "export function syncFiles() {\n  return 1\n}\n",
        "src/core/other.js": "import { syncFiles } from './sync.js'\nexport const run = () => syncFiles()\n",
        "src/cli.js": "#!/usr/bin/env node\nimport './core/other.js'\n",
        "test/sync.test.js": "import { syncFiles } from '../src/core/sync.js'\ntest('sync', () => syncFiles())\n",
        "knowledgebase/context-history/codebase-map.md":
            "# Codebase map\n\n| Repo | Purpose |\n|---|---|\n| `pkg` | Billing service for invoices |\n",
        "node_modules/dep/index.js": "function syncFiles() {}\n",
        ".env": "SYNC_TOKEN=abc\n",
        "assets/blob.bin": b"sync\x00\x01\x02binary",
        "package-lock.json": '{"name": "sync"}\n',
        "dist/bundle.min.js": "var sync=1\n",
    })
    make_repo(tmp_path / "pkg", {
        "lib/invoice.py": "def sync_invoice():\n    return 1\n",
        "tests/test_invoice.py": "from lib.invoice import sync_invoice\n",
    })
    (ws / "codebase-symlinks").mkdir()
    (ws / "codebase-symlinks" / "pkg").symlink_to(tmp_path / "pkg")
    git(ws, "add", "-f", "-A")
    return ws


def test_filename_match_outranks_content_only_match(workspace: Path) -> None:
    data = out(run(workspace, "find", "sync"))
    ranked = paths(data)
    assert ranked[0] == "ws:src/core/sync.js"
    assert ranked.index("ws:src/core/sync.js") < ranked.index("ws:README.md")
    assert data["files"][0]["lines"][0]["text"].startswith("export function syncFiles")


def test_search_covers_linked_codebases_with_labels(workspace: Path) -> None:
    data = out(run(workspace, "find", "invoice"))
    assert paths(data)[0] == "pkg:lib/invoice.py"
    assert {r["name"]: r["label"] for r in data["repos"]}["pkg"] == "Billing service for invoices"
    assert any(r["name"] == "ws" for r in data["repos"])


@pytest.mark.parametrize("hidden", [
    "node_modules/dep/index.js", ".env", "assets/blob.bin", "package-lock.json", "dist/bundle.min.js",
])
def test_ignored_binary_and_secret_files_are_never_returned(workspace: Path, hidden: str) -> None:
    data = out(run(workspace, "find", "sync"))
    assert f"ws:{hidden}" not in paths(data) + paths(data, "tests")
    assert hidden not in json.dumps(data)


def test_symlink_pointing_outside_repo_is_skipped(workspace: Path, tmp_path: Path) -> None:
    secret = tmp_path / "outside.txt"
    secret.write_text("sync secret contents\n")
    (workspace / "leak.txt").symlink_to(secret)
    git(workspace, "add", "-f", "-A")
    data = out(run(workspace, "find", "sync"))
    assert "leak.txt" not in json.dumps(data) and "secret contents" not in json.dumps(data)


def test_tests_entry_points_and_references_are_reported(workspace: Path) -> None:
    data = out(run(workspace, "find", "sync"))
    assert "ws:test/sync.test.js" in paths(data, "tests")
    assert "ws:src/cli.js" in paths(data, "entry_points")
    assert "ws:src/core/other.js" in paths(data, "references")


def test_results_are_capped_and_paginated(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "big", {f"src/widget_{i:02d}.py": "widget = 1\n" for i in range(25)})
    first = out(run(repo, "find", "widget", "--limit", "10"))
    assert len(first["files"]) == 10 and first["total"] == 25 and first["next_offset"] == 10
    last = out(run(repo, "find", "widget", "--limit", "10", "--offset", "20"))
    assert len(last["files"]) == 5 and last["next_offset"] is None
    assert not set(paths(first)) & set(paths(last))
    err(run(repo, "find", "widget", "--limit", "500"), 2)


def test_no_match_suggests_close_terms(workspace: Path) -> None:
    data = out(run(workspace, "find", "syncc"))
    assert data["files"] == [] and data["total"] == 0
    assert "sync" in data["suggestions"]


def test_regex_metacharacters_are_literal(workspace: Path) -> None:
    assert out(run(workspace, "find", "sync.*"))["total"] == 0


@pytest.mark.parametrize("term", ["", "a" * 100, "bad\x07term"])
def test_invalid_terms_are_rejected(workspace: Path, term: str) -> None:
    err(run(workspace, "find", term), 2)


def test_find_requires_a_term(workspace: Path) -> None:
    assert run(workspace, "find").returncode == 2


def test_outside_a_git_repo_is_reported(tmp_path: Path) -> None:
    plain = tmp_path / "plain"
    plain.mkdir()
    err(run(plain, "find", "x"), 1)


def test_root_option_must_be_a_directory(workspace: Path) -> None:
    err(run(workspace, "--root", str(workspace / "README.md"), "find", "sync"), 2)


def stack_of(repo: Path) -> dict[str, Any]:
    return out(run(repo, "stack"))


def first_repo(data: dict[str, Any]) -> dict[str, Any]:
    repo: dict[str, Any] = data["repos"][0]
    return repo


NODE_FILES: dict[str, str | bytes] = {
    "package.json": json.dumps({
        "dependencies": {"react": "^18", "express": "^4"},
        "devDependencies": {"jest": "^29", "vite": "^5"},
        "scripts": {"test": "jest --ci", "build": "vite build"},
    }),
    "package-lock.json": "{}",
    "src/app.tsx": "export const App = () => null\n",
    "src/server.js": "const express = require('express')\n",
    ".github/workflows/ci.yml": "name: ci\n",
}
PYTHON_FILES: dict[str, str | bytes] = {
    "pyproject.toml": '[project]\ndependencies = ["textual>=0.80", "Django>=5"]\n[dependency-groups]\ndev = ["pytest>=8"]\n'
                      "[tool.pytest.ini_options]\ntestpaths = [\"tests\"]\n",
    "uv.lock": "",
    "src/app.py": "x = 1\n",
    "tests/test_app.py": "def test_x(): pass\n",
}


def test_stack_detects_node_project(tmp_path: Path) -> None:
    repo = first_repo(stack_of(make_repo(tmp_path / "node", NODE_FILES)))
    assert repo["detected"] is True
    assert [m["path"] for m in repo["manifests"]] == ["package.json"]
    assert {"TypeScript", "JavaScript"} <= {lang["name"] for lang in repo["languages"]}
    assert {"react", "express", "vite"} <= set(repo["frameworks"])
    assert {"name": "jest", "command": "jest --ci", "source": "package.json"} in repo["test_runners"]
    assert repo["package_managers"] == ["npm"] and repo["ci"] == [".github/workflows/ci.yml"]


def test_stack_detects_python_project(tmp_path: Path) -> None:
    repo = first_repo(stack_of(make_repo(tmp_path / "py", PYTHON_FILES)))
    assert repo["languages"][0] == {"name": "Python", "files": 2}
    assert {"textual", "django"} <= set(repo["frameworks"])
    assert any(r["name"] == "pytest" for r in repo["test_runners"])
    assert repo["package_managers"] == ["uv"]


def test_stack_handles_mixed_monorepo(tmp_path: Path) -> None:
    files: dict[str, str | bytes] = {f"web/{k}": v for k, v in NODE_FILES.items() if not k.startswith(".github")}
    files.update({f"api/{k}": v for k, v in PYTHON_FILES.items()})
    files["go.mod"] = "module example.com/x\n\ngo 1.22\n\nrequire github.com/gin-gonic/gin v1.9.0\n"
    files["main.go"] = "package main\n"
    repo = first_repo(stack_of(make_repo(tmp_path / "mixed", files)))
    assert {m["path"] for m in repo["manifests"]} == {"web/package.json", "api/pyproject.toml", "go.mod"}
    assert {"react", "textual", "gin"} <= set(repo["frameworks"])
    assert {r["name"] for r in repo["test_runners"]} >= {"jest", "pytest", "go test"}
    assert {lang["name"] for lang in repo["languages"]} >= {"Python", "TypeScript", "Go"}


def test_stack_reports_undetected_for_empty_project(tmp_path: Path) -> None:
    repo = first_repo(stack_of(make_repo(tmp_path / "empty", {"README.md": "# hi\n"})))
    assert repo["detected"] is False
    assert repo["manifests"] == [] and repo["languages"] == [] and repo["test_runners"] == []


def test_stack_covers_linked_codebases(workspace: Path) -> None:
    data = stack_of(workspace)
    assert [r["name"] for r in data["repos"]] == ["ws", "pkg"]
    assert data["repos"][1]["linked"] is True and data["repos"][1]["label"] == "Billing service for invoices"
    assert data["repos"][1]["languages"][0]["name"] == "Python"


def test_stack_ignores_vendored_manifests_and_survives_bad_json(tmp_path: Path) -> None:
    repo = first_repo(stack_of(make_repo(tmp_path / "r", {
        "package.json": "{not json", "node_modules/dep/package.json": '{"dependencies": {"react": "1"}}',
    })))
    assert [m["path"] for m in repo["manifests"]] == ["package.json"]
    assert "react" not in repo["frameworks"] and repo["warnings"]


def test_stack_outside_a_git_repo_is_reported(tmp_path: Path) -> None:
    plain = tmp_path / "plain"
    plain.mkdir()
    err(run(plain, "stack"), 1)
