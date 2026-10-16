"""Regenerate the README screenshots from a fictional demo project (no real paths, names or sessions).

Run with tandem's interpreter from the repository root:

    cd tandem && uv run python ../docs/screenshots/make_screenshots.py

It builds a throwaway workspace ("acme-workspace") with stand-in `claude`/`agy` programs that only print
sample text, runs the real tandem against it in a private tmux server, captures the screen and writes SVGs.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from ansi_svg import render  # noqa: E402

OUT = Path(__file__).resolve().parents[1] / "images"
COORD = Path(__file__).resolve().parents[2] / "templates/skills/agent-coordination/scripts/coord.py"
COLS, ROWS = 150, 50
GIT = ["git", "-c", "user.email=demo@example.com", "-c", "user.name=Demo"]
OUTER = f"shots-outer-{os.getpid()}"

PLANNER_SCREEN = """\
\x1b[38;5;208m ▐▛███▜▌\x1b[0m  \x1b[1mClaude Code\x1b[0m
\x1b[38;5;208m▝▜█████▛▘\x1b[0m \x1b[2mopus · sample session\x1b[0m

\x1b[2m>\x1b[0m Break the checkout redesign into tasks

\x1b[32m●\x1b[0m Checking active and planned work first.
  \x1b[2m⎿ agent-coordination: 2 tasks claimed by implementers\x1b[0m
\x1b[32m●\x1b[0m Drafting t-004 (order summary) and t-005 (delivery)
  \x1b[2m⎿ Affected Paths: src/checkout/summary.tsx\x1b[0m

\x1b[2m(sample output printed by a stand-in program)\x1b[0m
"""
IMPLEMENTER_SCREEN = """\
\x1b[38;5;208m ▐▛███▜▌\x1b[0m  \x1b[1mClaude Code\x1b[0m
\x1b[38;5;208m▝▜█████▛▘\x1b[0m \x1b[2msonnet · sample session\x1b[0m

\x1b[2m>\x1b[0m /spec-implement p-001 t-002

\x1b[32m●\x1b[0m Claimed p-001-acme-web t-002 on branch feat/checkout-flow
\x1b[32m●\x1b[0m Writing the failing test first
  \x1b[2m⎿ tests/checkout/payment.test.ts\x1b[0m
\x1b[33m●\x1b[0m Question queued for you (not blocking other work)

\x1b[2m(sample output printed by a stand-in program)\x1b[0m
"""
AGY_SCREEN = "\x1b[34m◆ Antigravity\x1b[0m  \x1b[2msample session\x1b[0m\n\n\x1b[2m(sample output printed by a stand-in program)\x1b[0m\n"


def sh(*argv: str, cwd: Path | None = None, env: dict[str, str] | None = None) -> str:
    done = subprocess.run(list(argv), cwd=cwd, env=env, capture_output=True, text=True)
    return done.stdout


def make_repo(path: Path, branches: list[str], checked_out: str, worktrees: dict[str, str]) -> None:
    path.mkdir(parents=True)
    sh(*GIT, "init", "-q", "-b", "trunk", cwd=path)
    (path / "README.md").write_text("# demo\n")
    sh(*GIT, "add", "-A", cwd=path)
    sh(*GIT, "commit", "-qm", "initial", cwd=path)
    for b in branches:
        sh(*GIT, "branch", b, cwd=path)
    for branch, folder in worktrees.items():
        sh(*GIT, "worktree", "add", "-q", str(path.parent / folder), branch, cwd=path)
    sh(*GIT, "checkout", "-q", checked_out, cwd=path)


def build_workspace(root: Path) -> Path:
    ws = root / "acme-workspace"
    (ws / "specs" / "p-001-acme-web").mkdir(parents=True)
    (ws / "specs" / "p-002-acme-api").mkdir(parents=True)
    (ws / "codebase-symlinks").mkdir()
    tasks = [("t-001", "Cart totals and rounding", "Done"), ("t-002", "Payment step with saved cards", "In Progress"),
             ("t-003", "Cart persistence across logins", "In Progress"), ("t-004", "Order summary page", "Pending"),
             ("t-005", "Delivery: commit, PR, review, merge", "Pending")]
    (ws / "specs/p-001-acme-web/impl.md").write_text("# p-001 acme-web\n\n## Tasks\n" + "".join(
        f"- [{'x' if s == 'Done' else ' '}] {i}: {t}\n" for i, t, s in tasks)
        + "\n## Timeline\n" + "".join(f"- **{i}**: {t} ({s})\n" for i, t, s in tasks))
    api = [("t-001", "Payment intents endpoint", "Done"), ("t-002", "Webhook signature checks", "Done"),
           ("t-003", "Rate limiting", "In Progress"), ("t-004", "Delivery: commit, PR, review, merge", "Pending")]
    (ws / "specs/p-002-acme-api/impl.md").write_text("# p-002 acme-api\n\n## Tasks\n" + "".join(
        f"- [{'x' if s == 'Done' else ' '}] {i}: {t}\n" for i, t, s in api)
        + "\n## Timeline\n" + "".join(f"- **{i}**: {t} ({s})\n" for i, t, s in api))
    for phase, items in (("p-001-acme-web", tasks), ("p-002-acme-api", api)):
        for i, _t, s in items:
            (ws / "specs" / phase / f"{i}.md").write_text(f"# {i}\n\n## Status\n- [x] {s}\n")
    (ws / "specs/in-progress.md").write_text(
        "# In Progress\n\n### p-001-acme-web — Checkout redesign\n"
        "- **Ongoing:** t-002 payment step, t-003 cart persistence\n- **Next step:** t-004 order summary\n")
    sh(*GIT, "init", "-q", "-b", "trunk", cwd=ws)
    (ws / "AGENTS.md").write_text("# Acme workspace\n")
    sh(*GIT, "add", "-A", cwd=ws)
    sh(*GIT, "commit", "-qm", "initial", cwd=ws)
    sh(*GIT, "checkout", "-q", "-b", "docs/checkout-specs", cwd=ws)
    make_repo(root / "acme-web", ["feat/checkout-flow", "fix/cart-total", "docs/readme", "chore/deps"],
              "trunk", {"feat/checkout-flow": "acme-web-checkout"})
    make_repo(root / "acme-api", ["feat/payments", "fix/rate-limit"], "feat/payments",
              {"fix/rate-limit": "acme-api-ratelimit"})
    for name in ("acme-web", "acme-api"):
        (ws / "codebase-symlinks" / name).symlink_to(root / name)
    return ws


def fake_clis(root: Path) -> Path:
    bin_dir = root / "bin"
    bin_dir.mkdir()
    (bin_dir / "planner.txt").write_text(PLANNER_SCREEN)
    (bin_dir / "implementer.txt").write_text(IMPLEMENTER_SCREEN)
    (bin_dir / "agy.txt").write_text(AGY_SCREEN)
    (bin_dir / "claude").write_text(
        f'#!/bin/sh\ncat "{bin_dir}/${{TANDEM_ROLE:-planner}}.txt"\nexec sleep 3600\n')
    (bin_dir / "agy").write_text(
        f'#!/bin/sh\nif [ "$1" = models ]; then printf "gemini-3.8-flash-high\\tGemini 3.8 Flash (High)\\n"; exit 0; fi\n'
        f'cat "{bin_dir}/agy.txt"\nexec sleep 3600\n')
    for f in ("claude", "agy"):
        (bin_dir / f).chmod(0o755)
    return bin_dir


def capture(target: str = "") -> str:
    return sh("tmux", "-L", OUTER, "capture-pane", "-e", "-p", "-t", target or "demo")


def save(name: str, title: str = "") -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"{name}.svg").write_text(render(capture(), COLS, ROWS, title))
    print("wrote", OUT / f"{name}.svg")


def main() -> None:
    from tandem import state
    from tandem.adapters.base import AgentSpec
    from tandem.quota import ClaudeQuota, Window, write_feed
    from tandem.tmux.client import socket_for

    root = Path(tempfile.mkdtemp(prefix="tandem-demo-")).resolve()
    ws = build_workspace(root)
    bin_dir = fake_clis(root)
    xdg = root / "state"
    (xdg / "tandem").mkdir(parents=True)
    (xdg / "tandem" / "first-run-done").write_text("0")  # no first-run help popup over the screenshot
    coord = ws / ".agent-workspace" / "coordination"
    now = time.time()
    state.save(state.state_path(ws), state.SavedState((
        AgentSpec("claude", "planner", "opus", 1), AgentSpec("agy", "planner", "gemini-3.8-flash-high", 2),
        AgentSpec("claude", "implementer", "sonnet", 1), AgentSpec("claude", "implementer", "sonnet", 2)),
        {"planner": 1, "implementer": 1}, saved_at=now))
    env = {**os.environ, "PATH": f"{bin_dir}:{os.environ['PATH']}", "XDG_STATE_HOME": str(xdg), "NO_COLOR": "",
           "TERM": "xterm-256color", "LANG": "en_US.UTF-8", "HOME": str(root)}
    env.pop("TMUX", None)
    py = sys.executable
    sock = socket_for(ws)
    try:
        sh("tmux", "-L", OUTER, "-f", "/dev/null", "new-session", "-d", "-x", str(COLS), "-y", str(ROWS), "-n", "demo",
           f"cd {ws} && {py} -m tandem --resume; sleep 5", env=env)
        time.sleep(8)
        for who, role, model in (("implementer-1", "implementer", "sonnet"), ("implementer-2", "implementer", "sonnet"),
                                 ("planner-1", "planner", "opus")):
            cenv = {**env, "TANDEM_SESSION_ID": who, "TANDEM_ROLE": role, "TANDEM_COORD_DIR": str(coord), "TANDEM_CLI": "claude"}
            sh(py, str(COORD), "register", "--model", model, cwd=ws, env=cenv)
        c1 = {**env, "TANDEM_SESSION_ID": "implementer-1", "TANDEM_COORD_DIR": str(coord)}
        c2 = {**env, "TANDEM_SESSION_ID": "implementer-2", "TANDEM_COORD_DIR": str(coord)}
        sh(py, str(COORD), "claim", "p-001-acme-web", "t-002", "--branch", "feat/checkout-flow",
           "--worktree", "../acme-web-checkout", "--paths", "src/checkout/", cwd=ws, env=c1)
        sh(py, str(COORD), "claim", "p-001-acme-web", "t-003", "--branch", "fix/cart-total", "--paths", "src/cart/", cwd=ws, env=c2)
        sh(py, str(COORD), "ask", "Should the cart persist across logins?", "--task", "t-003",
           "--blocking-scope", "t-003 persistence", "--option", "Yes, store it server-side",
           "--option", "No, keep it per browser session", cwd=ws, env=c2)
        write_feed(coord, "planner-1", ClaudeQuota(Window(42, int(now) + 3 * 3600), Window(12, int(now) + 4 * 86400), now))
        time.sleep(4)
        panes = sh("tmux", "-L", sock, "list-panes", "-a", "-F", "#{pane_id} #{@tandem_role} #{@tandem_kind}").splitlines()
        status = next(p.split()[0] for p in panes if " status " in p + " ")
        # 1. overview
        sh("tmux", "-L", sock, "resize-pane", "-t", status, "-y", "16")
        time.sleep(3)
        save("overview", "tandem: planners left, implementers right, status below")
        # 2. status window, taller
        sh("tmux", "-L", sock, "resize-pane", "-t", status, "-y", "34")
        time.sleep(3)
        save("status", "Status tab: work in progress, repositories (branch → worktree), phases")
        # 3. queries tab
        sh("tmux", "-L", sock, "send-keys", "-t", status, "F6")
        time.sleep(3)
        save("queries", "Queries tab: questions your agents queued while they keep working")
        sh("tmux", "-L", sock, "send-keys", "-t", status, "Tab")
        # 4. help popup
        client = sh("tmux", "-L", sock, "list-clients", "-F", "#{client_name}").split()[0]
        subprocess.Popen(["tmux", "-L", sock, "display-popup", "-c", client, "-E", "-w", "70%", "-h", "75%",
                          f"env TANDEM_WORKSPACE={ws} {py} -m tandem help"], env=env,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(3)
        save("help", "F1 help")
        sh("tmux", "-L", sock, "display-popup", "-c", client, "-C")
    finally:
        sh("tmux", "-L", sock, "kill-server")
        sh("tmux", "-L", OUTER, "kill-server")
        shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    main()
