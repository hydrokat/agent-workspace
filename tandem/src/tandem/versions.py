import re
import shutil
import subprocess
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

_VERSION_RE = re.compile(r"(\d+(?:\.\d+)*)([a-z]?)")
_MIN_LINE_RE = re.compile(r"^[A-Z_]+=[0-9A-Za-z.\-]+$")


class InvalidMinVersionsError(ValueError):
    """Raised when min-versions.env contains a line outside the strict grammar."""


@dataclass(frozen=True)
class Version:
    parts: tuple[int, ...]
    suffix: str

    @property
    def text(self) -> str:
        return ".".join(str(p) for p in self.parts) + self.suffix


def parse_version(raw: str) -> Version | None:
    """Extract the first version-like token (e.g. ``3.3a``, ``2.1.301``) from tool output."""
    match = _VERSION_RE.search(raw)
    if match is None:
        return None
    parts = tuple(int(p) for p in match.group(1).split("."))
    return Version(parts, match.group(2))


def _padded(parts: tuple[int, ...], length: int) -> tuple[int, ...]:
    return parts + (0,) * (length - len(parts))


def is_at_least(actual: Version, minimum: Version) -> bool:
    """Return True when ``actual`` >= ``minimum``; missing components count as zero."""
    length = max(len(actual.parts), len(minimum.parts))
    return (_padded(actual.parts, length), actual.suffix) >= (
        _padded(minimum.parts, length),
        minimum.suffix,
    )


def parse_min_versions(path: Path) -> dict[str, str]:
    """Parse ``min-versions.env`` as data only; never evaluates content."""
    result: dict[str, str] = {}
    for number, line in enumerate(path.read_text().splitlines(), start=1):
        if not line.strip() or line.startswith("#"):
            continue
        if not _MIN_LINE_RE.match(line):
            raise InvalidMinVersionsError(f"{path.name}:{number}: invalid line")
        key, value = line.split("=", 1)
        result[key] = value
    return result


def _capture(argv: list[str]) -> tuple[int, str]:
    try:
        done = subprocess.run(argv, capture_output=True, text=True, timeout=10, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.SubprocessError):
        return 1, ""
    return done.returncode, done.stdout


def installed_version(cli: str, which: Callable[[str], str | None] = shutil.which,
                      runner: Callable[[list[str]], tuple[int, str]] = _capture) -> str:
    """The version `cli --version` reports, or an empty string when it is missing or does not answer."""
    path = which(cli)
    if path is None:
        return ""
    code, out = runner([path, "--version"])
    parsed = parse_version(out) if code == 0 else None
    return parsed.text if parsed else ""


def verify_release_versions(tag: str, repo_root: Path) -> tuple[bool, str]:
    import json
    import tomllib
    import tandem

    if not tag.startswith("v"):
        return False, f"tag '{tag}' does not start with 'v'"
    expected = tag[1:]

    pkg_json = repo_root / "package.json"
    if not pkg_json.is_file():
        return False, f"package.json not found at {pkg_json}"
    try:
        node_version = json.loads(pkg_json.read_text())["version"]
    except (json.JSONDecodeError, KeyError) as e:
        return False, f"failed to read package.json version: {e}"
    if node_version != expected:
        return False, f"package.json version '{node_version}' != tag version '{expected}'"

    pyproject = repo_root / "tandem" / "pyproject.toml"
    if not pyproject.is_file():
        return False, f"pyproject.toml not found at {pyproject}"
    try:
        py_version = tomllib.loads(pyproject.read_text())["project"]["version"]
    except (tomllib.TOMLDecodeError, KeyError) as e:
        return False, f"failed to read pyproject.toml version: {e}"
    if py_version != expected:
        return False, f"pyproject.toml version '{py_version}' != tag version '{expected}'"

    if tandem.__version__ != expected:
        return False, f"tandem.__version__ '{tandem.__version__}' != tag version '{expected}'"

    return True, f"all versions match {expected}"
