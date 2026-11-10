import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/symlink/scripts/symlink.py"


def run(root: Path, *args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(SCRIPT), "--root", str(root), *args],
                          capture_output=True, text=True, cwd=cwd or root)


def out(proc: subprocess.CompletedProcess[str], code: int = 0) -> dict[str, Any]:
    assert proc.returncode == code, proc.stdout + proc.stderr
    data: dict[str, Any] = json.loads(proc.stdout)
    return data


def make_layout(tmp_path: Path) -> tuple[Path, Path]:
    workspace = tmp_path / "workspace"
    workspace.mkdir()
    api = tmp_path / "api-service"
    api.mkdir()
    (api / "README.md").write_text("# API\n\nOrders and billing API.\n")
    return workspace, api


def map_text(workspace: Path) -> str:
    return (workspace / "knowledgebase/context-history/codebase-map.md").read_text()


def test_creates_relative_link_and_refreshes_map(tmp_path: Path) -> None:
    workspace, api = make_layout(tmp_path)
    data = out(run(workspace, "../api-service"))
    link = workspace / "codebase-symlinks/api-service"
    assert data["status"] == "created"
    assert data["link"] == "codebase-symlinks/api-service"
    assert os.readlink(link) == "../../api-service"
    assert link.resolve() == api.resolve()
    assert data["map"] == "knowledgebase/context-history/codebase-map.md"
    assert "Orders and billing API." in map_text(workspace)


def test_custom_name_and_absolute_path(tmp_path: Path) -> None:
    workspace, api = make_layout(tmp_path)
    data = out(run(workspace, str(api), "--name", "api"))
    assert data["status"] == "created"
    assert (workspace / "codebase-symlinks/api").resolve() == api.resolve()


def test_relink_same_target_is_preserved(tmp_path: Path) -> None:
    workspace, _ = make_layout(tmp_path)
    out(run(workspace, "../api-service"))
    assert out(run(workspace, "../api-service"))["status"] == "preserved"


def test_target_already_linked_under_other_name(tmp_path: Path) -> None:
    workspace, _ = make_layout(tmp_path)
    out(run(workspace, "../api-service"))
    data = out(run(workspace, "../api-service", "--name", "api"))
    assert data == {**data, "status": "already-linked", "link": "codebase-symlinks/api-service"}
    assert not os.path.lexists(workspace / "codebase-symlinks/api")


def test_name_clash_is_refused_without_overwrite(tmp_path: Path) -> None:
    workspace, _ = make_layout(tmp_path)
    other = tmp_path / "other"
    other.mkdir()
    out(run(workspace, "../api-service"))
    data = out(run(workspace, str(other), "--name", "api-service"), 1)
    assert data["status"] == "refused"
    assert "choose another --name" in data["reason"]
    assert os.readlink(workspace / "codebase-symlinks/api-service") == "../../api-service"


def test_refusals(tmp_path: Path) -> None:
    workspace, _ = make_layout(tmp_path)
    (workspace / "src").mkdir()
    (tmp_path / "file.txt").write_text("x")
    cases = {
        "missing": ["../nope"],
        "file": ["../file.txt"],
        "self": ["."],
        "parent": [".."],
        "inside": ["src"],
        "bad name": ["../api-service", "--name", "a/b"],
        "dot name": ["../api-service", "--name", ".."],
    }
    for label, args in cases.items():
        data = out(run(workspace, *args), 1)
        assert data["status"] == "refused", label
    assert not (workspace / "codebase-symlinks").exists()


def test_dry_run_changes_nothing(tmp_path: Path) -> None:
    workspace, _ = make_layout(tmp_path)
    data = out(run(workspace, "../api-service", "--dry-run"))
    assert data["status"] == "would-create"
    assert not (workspace / "codebase-symlinks").exists()
    assert not (workspace / "knowledgebase").exists()


def test_siblings_lists_candidates_with_link_state(tmp_path: Path) -> None:
    workspace, api = make_layout(tmp_path)
    (tmp_path / "web").mkdir()
    out(run(workspace, "../api-service"))
    siblings = {s["name"]: s for s in out(run(workspace, "--siblings"))["siblings"]}
    assert set(siblings) == {"api-service", "web"}
    assert siblings["api-service"]["linked_as"] == "api-service"
    assert siblings["api-service"]["purpose"] == "Orders and billing API."
    assert siblings["web"]["linked_as"] is None


def test_requires_a_path(tmp_path: Path) -> None:
    workspace, _ = make_layout(tmp_path)
    assert out(run(workspace), 1)["status"] == "refused"
