import re
from dataclasses import dataclass, field
from pathlib import Path

from tandem.sanitize import clean

MAX_BYTES = 1_000_000
_PHASE_DIR = re.compile(r"^(p-\d{3}-[a-z0-9-]+|phase-\d{3})$")
_TASK_LINE = re.compile(r"^- \[( |x|X)\] ((?:t|task)-\d{3}):\s*(.*)$")
_TIMELINE = re.compile(r"^- \*\*((?:t|task)-\d{3})\*\*:.*\((Pending|In Progress|Blocked|In Review|Done)\)\s*$")
_STATUS_LINE = re.compile(r"^- \[[xX]\] (Pending|In Progress|Blocked|In Review|Done)\b")


@dataclass(frozen=True)
class Task:
    id: str
    title: str
    status: str


@dataclass(frozen=True)
class Phase:
    name: str
    tasks: list[Task] = field(default_factory=list)
    warning: str = ""

    @property
    def total(self) -> int:
        return len(self.tasks)

    @property
    def done(self) -> int:
        return sum(1 for t in self.tasks if t.status == "Done")


def _read(path: Path) -> str | None:
    try:
        with path.open("rb") as handle:
            return handle.read(MAX_BYTES).decode("utf-8", errors="replace")
    except OSError:
        return None


def _task_status(text: str | None) -> str | None:
    if text is None:
        return None
    in_status = False
    for line in text.splitlines():
        if line.startswith("## "):
            in_status = line.strip() == "## Status"
        elif in_status:
            match = _STATUS_LINE.match(line.strip())
            if match:
                return match.group(1)
    return None


def _load_phase(directory: Path) -> Phase:
    impl = _read(directory / "impl.md")
    if impl is None:
        return Phase(directory.name, warning=f"{directory.name}/impl.md missing or unreadable")
    timeline = {m.group(1): m.group(2) for line in impl.splitlines() if (m := _TIMELINE.match(line.strip()))}
    tasks: list[Task] = []
    for line in impl.splitlines():
        match = _TASK_LINE.match(line.strip())
        if not match:
            continue
        checked, task_id, title = match.groups()
        status = (_task_status(_read(directory / f"{task_id}.md")) or timeline.get(task_id)
                  or ("Done" if checked.lower() == "x" else "Pending"))
        tasks.append(Task(task_id, clean(title).strip(), status))
    return Phase(directory.name, tasks)


def load_phases(workspace: Path) -> list[Phase]:
    specs = workspace / "specs"
    if not specs.is_dir() or specs.is_symlink():
        return []
    phases: list[Phase] = []
    for child in sorted(specs.iterdir()):
        if child.is_dir() and not child.is_symlink() and _PHASE_DIR.match(child.name):
            phases.append(_load_phase(child))
    return phases
