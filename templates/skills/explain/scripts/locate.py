#!/usr/bin/env python3
"""Locate the code behind a module, feature, or question in the repo and its linked codebases.

Stdlib only. JSON on stdout. Exit codes: 0 ok, 1 not a git repo, 2 invalid input.

Commands (read-only):
  stack
      {"repos": [{name, linked, label, detected, manifests: [{path, kind}], languages: [{name, files}],
       frameworks[], package_managers[], test_runners: [{name, command, source}], ci[], warnings[]}]}
      Detects what each repo is built with. `detected` is false when no manifest or language is found.
  find <term>... [--limit N] [--offset N]
      {"query", "repos": [{name, label, linked}], "total", "offset", "limit", "next_offset",
       "files": [{repo, path, score, lines: [{line, text}]}], "tests": [...], "entry_points": [...],
       "references": [{repo, path, line, text}], "areas": [{area, count}], "suggestions": [...],
       "truncated"}
      Searches tracked files of the current repo and of each repo under codebase-symlinks/.
      Skips binary, vendored, generated, oversized, and secret files, and symlinks that leave their repo.
"""
import argparse
import difflib
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, NoReturn

MAX_FILE_BYTES = 1_000_000
MAX_SCANNED_FILES = 20_000
MAX_TERMS = 8
MAX_TERM_LENGTH = 64
MAX_LIMIT = 50
DEFAULT_LIMIT = 10
LINES_PER_FILE = 5
LINE_WIDTH = 160
CODEBASE_MAP = Path("knowledgebase/context-history/codebase-map.md")

IGNORED_DIRS = {
    "node_modules", "vendor", ".venv", "venv", "dist", "build", ".claude", ".gemini", ".git",
    "__pycache__", ".next", "target", "coverage", ".tox", ".mypy_cache", ".pytest_cache",
}
LOCKFILES = {
    "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "poetry.lock", "uv.lock", "Cargo.lock",
    "Gemfile.lock", "composer.lock", "go.sum", "bun.lockb",
}
SECRET_NAME_RE = re.compile(
    r"^(\.env(\..+)?|.*\.(pem|key|p12|pfx|keystore|jks)|id_(rsa|ed25519|ecdsa|dsa).*|credentials.*|"
    r"secrets?\..*|\.npmrc|\.netrc)$", re.IGNORECASE,
)
SAFE_ENV_RE = re.compile(r"\.(example|sample|template)$", re.IGNORECASE)
GENERATED_RE = re.compile(r"\.(min\.(js|css)|map|lock)$|\.d\.ts$", re.IGNORECASE)
TEST_DIRS = {"test", "tests", "__tests__", "spec"}
TEST_NAME_RE = re.compile(r"(^test_|_test\.|\.test\.|\.spec\.|_spec\.)")
ENTRY_STEMS = {"main", "index", "cli", "app", "__main__", "server", "run", "manage", "wsgi", "asgi"}
DEFINITION_RE = re.compile(
    r"^\s*(export\s+|pub\s+|public\s+|private\s+|async\s+|static\s+|default\s+)*"
    r"(def|class|function|func|fn|interface|type|struct|enum|trait|const|let|var)\b",
)
IMPORT_LINE_RE = re.compile(r"\b(import|require|from|use|include|source|load)\b")
MAIN_GUARD_RE = re.compile(r"__name__\s*==\s*['\"]__main__['\"]")
CTRL_RE = re.compile(r"[\x00-\x1f\x7f-\x9f]")


def fail(message: str, code: int = 2, **extra: Any) -> NoReturn:
    print(json.dumps({"error": message, **extra}))
    sys.exit(code)


def emit(data: dict[str, Any]) -> None:
    print(json.dumps(data, ensure_ascii=False))


def git_output(repo: Path, *args: str) -> str | None:
    try:
        proc = subprocess.run(
            ["git", "-C", str(repo), *args], capture_output=True, text=True, timeout=30, check=True,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    return proc.stdout


class Repo:
    def __init__(self, name: str, root: Path, linked: bool) -> None:
        self.name = name
        self.root = root
        self.linked = linked
        self.label: str | None = None


def top_level(path: Path) -> Path | None:
    out = git_output(path, "rev-parse", "--show-toplevel")
    return Path(out.strip()).resolve() if out and out.strip() else None


def codebase_label(map_text: str, name: str) -> str | None:
    for line in map_text.splitlines():
        if name not in line:
            continue
        if line.lstrip().startswith("|"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if len(cells) > 1 and name == cells[0].strip("`* "):
                return cells[-1].strip("`* ")[:120] or None
        else:
            tail = re.split(rf"`?{re.escape(name)}`?\**\s*(?:—|–|-|:)\s*", line, maxsplit=1)
            if len(tail) == 2 and tail[1].strip():
                return tail[1].strip("`* ")[:120]
    return None


def discover_repos(start: Path) -> list[Repo]:
    main_root = top_level(start)
    if main_root is None:
        fail("not inside a git repository", 1)
    repos = [Repo(main_root.name, main_root, False)]
    links = main_root / "codebase-symlinks"
    if links.is_dir():
        for entry in sorted(links.iterdir()):
            resolved = entry.resolve()
            root = top_level(resolved) if resolved.is_dir() else None
            if root is not None and root == resolved and all(r.root != root for r in repos):
                repos.append(Repo(entry.name, root, True))
    map_path = main_root / CODEBASE_MAP
    if map_path.is_file():
        map_text = map_path.read_text(encoding="utf-8", errors="replace")
        for repo in repos[1:]:
            repo.label = codebase_label(map_text, repo.name)
    return repos


def tracked_files(repo: Repo) -> list[str]:
    listing = git_output(repo.root, "ls-files", "-z")
    return [rel for rel in (listing or "").split("\0") if rel]


def skip_path(rel: str) -> bool:
    parts = rel.split("/")
    name = parts[-1]
    if any(part in IGNORED_DIRS for part in parts[:-1]):
        return True
    if name in LOCKFILES or GENERATED_RE.search(name):
        return True
    return bool(SECRET_NAME_RE.match(name)) and not SAFE_ENV_RE.search(name)


def read_candidate(repo: Repo, rel: str) -> str | None:
    path = repo.root / rel
    try:
        if path.is_symlink() and not path.resolve().is_relative_to(repo.root):
            return None
        if not path.is_file() or path.stat().st_size > MAX_FILE_BYTES:
            return None
        data = path.read_bytes()
    except OSError:
        return None
    if b"\x00" in data[:8192]:
        return None
    return data.decode("utf-8", errors="replace")


def is_test(rel: str) -> bool:
    parts = rel.split("/")
    return any(p in TEST_DIRS for p in parts[:-1]) or bool(TEST_NAME_RE.search(parts[-1]))


def score_file(rel: str, lines: list[str], terms: list[str]) -> tuple[int, list[dict[str, Any]]]:
    name = rel.rsplit("/", 1)[-1].lower()
    stem = name.split(".")[0]
    path_lower = rel.lower()
    score = 0
    matched_terms: set[str] = set()
    for term in terms:
        if stem == term:
            score += 10
        elif term in name:
            score += 6
        elif term in path_lower:
            score += 3
        else:
            continue
        matched_terms.add(term)
    content_score = definition_score = 0
    hits: list[dict[str, Any]] = []
    for number, line in enumerate(lines, 1):
        lowered = line.lower()
        found = [t for t in terms if t in lowered]
        if not found:
            continue
        matched_terms.update(found)
        content_score += 1
        if DEFINITION_RE.match(line):
            definition_score += 4
        if len(hits) < LINES_PER_FILE:
            hits.append({"line": number, "text": line.strip()[:LINE_WIDTH]})
    score += min(content_score, 20) + min(definition_score, 12)
    if score and matched_terms == set(terms) and len(terms) > 1:
        score += 5
    return score, hits


def check_terms(raw: list[str]) -> list[str]:
    if len(raw) > MAX_TERMS:
        fail(f"at most {MAX_TERMS} terms")
    terms = []
    for term in raw:
        if not term.strip() or len(term) > MAX_TERM_LENGTH or CTRL_RE.search(term):
            fail("invalid search term")
        terms.append(term.strip().lower())
    return terms


def suggest(terms: list[str], vocabulary: set[str]) -> list[str]:
    found: list[str] = []
    for term in terms:
        for match in difflib.get_close_matches(term, sorted(vocabulary), n=3, cutoff=0.7):
            if match not in found:
                found.append(match)
    return found


def find(args: argparse.Namespace) -> None:
    terms = check_terms(args.terms)
    if not 1 <= args.limit <= MAX_LIMIT or args.offset < 0:
        fail(f"--limit must be 1..{MAX_LIMIT} and --offset >= 0")
    start = Path(args.root) if args.root else Path.cwd()
    if not start.is_dir():
        fail("--root must be a directory")
    repos = discover_repos(start)
    scanned = 0
    truncated = False
    ranked: list[dict[str, Any]] = []
    tests: list[dict[str, Any]] = []
    contents: dict[tuple[str, str], list[str]] = {}
    vocabulary: set[str] = set()
    entry_candidates: list[dict[str, str]] = []
    for repo in repos:
        for rel in tracked_files(repo):
            if skip_path(rel):
                continue
            vocabulary.update(w for w in re.split(r"[^a-z0-9]+", rel.lower()) if len(w) > 2)
            if scanned >= MAX_SCANNED_FILES:
                truncated = True
                break
            text = read_candidate(repo, rel)
            if text is None:
                continue
            scanned += 1
            lines = text.splitlines()
            contents[(repo.name, rel)] = lines
            stem = rel.rsplit("/", 1)[-1].lower().split(".")[0]
            if not is_test(rel) and (
                stem in ENTRY_STEMS or text.startswith("#!") or MAIN_GUARD_RE.search(text)
            ):
                entry_candidates.append({"repo": repo.name, "path": rel})
            score, hits = score_file(rel, lines, terms)
            if score:
                item = {"repo": repo.name, "path": rel, "score": score, "lines": hits}
                (tests if is_test(rel) else ranked).append(item)
    order = {repo.name: index for index, repo in enumerate(repos)}

    def rank_key(item: dict[str, Any]) -> tuple[int, int, str]:
        return -item["score"], order[item["repo"]], item["path"]

    ranked.sort(key=rank_key)
    tests.sort(key=rank_key)
    page = ranked[args.offset:args.offset + args.limit]
    next_offset = args.offset + args.limit if args.offset + args.limit < len(ranked) else None
    emit({
        "query": terms,
        "repos": [{"name": r.name, "label": r.label, "linked": r.linked} for r in repos],
        "total": len(ranked), "offset": args.offset, "limit": args.limit, "next_offset": next_offset,
        "files": page, "tests": tests[:10],
        "entry_points": entry_points(entry_candidates, ranked[:5]),
        "references": references(ranked[:3], contents),
        "areas": areas(ranked[:20]),
        "suggestions": suggest(terms, vocabulary) if not ranked and not tests else [],
        "truncated": truncated,
    })


def entry_points(candidates: list[dict[str, str]], top: list[dict[str, Any]]) -> list[dict[str, str]]:
    found = []
    for candidate in candidates:
        directory = candidate["path"].rpartition("/")[0]
        for item in top:
            if item["repo"] != candidate["repo"]:
                continue
            target_dir = item["path"].rpartition("/")[0]
            if directory == "" or target_dir == directory or target_dir.startswith(directory + "/"):
                found.append(candidate)
                break
    return found[:5]


def references(top: list[dict[str, Any]], contents: dict[tuple[str, str], list[str]]) -> list[dict[str, Any]]:
    found: list[dict[str, Any]] = []
    seen: set[tuple[str, str]] = set()
    for item in top:
        stem = item["path"].rsplit("/", 1)[-1].split(".")[0]
        pattern = re.compile(rf"\b{re.escape(stem)}\b")
        for (repo, rel), lines in contents.items():
            if repo != item["repo"] or rel == item["path"] or is_test(rel) or (repo, rel) in seen:
                continue
            for number, line in enumerate(lines, 1):
                if IMPORT_LINE_RE.search(line) and pattern.search(line):
                    seen.add((repo, rel))
                    found.append({"repo": repo, "path": rel, "line": number, "text": line.strip()[:LINE_WIDTH]})
                    break
    return found[:10]


def areas(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    counts: dict[str, int] = {}
    for item in items:
        segments = item["path"].split("/")[:-1][:2]
        area = f"{item['repo']}:{'/'.join(segments) or '.'}"
        counts[area] = counts.get(area, 0) + 1
    return [{"area": a, "count": c} for a, c in sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[:5]]


MANIFEST_KINDS = {
    "package.json": "npm", "pyproject.toml": "python", "requirements.txt": "python", "setup.py": "python",
    "Pipfile": "python", "go.mod": "go", "Cargo.toml": "cargo", "composer.json": "composer",
    "Gemfile": "bundler", "pubspec.yaml": "pub", "pom.xml": "maven", "build.gradle": "gradle",
    "build.gradle.kts": "gradle", "Package.swift": "swift", "mix.exs": "mix", "Dockerfile": "docker",
    "docker-compose.yml": "docker", "docker-compose.yaml": "docker", "Makefile": "make",
}
LOCK_MANAGERS = {
    "package-lock.json": "npm", "yarn.lock": "yarn", "pnpm-lock.yaml": "pnpm", "bun.lockb": "bun",
    "uv.lock": "uv", "poetry.lock": "poetry", "Pipfile.lock": "pipenv", "Cargo.lock": "cargo",
    "composer.lock": "composer", "Gemfile.lock": "bundler", "go.sum": "go modules", "pubspec.lock": "pub",
}
LANGUAGES = {
    "py": "Python", "js": "JavaScript", "jsx": "JavaScript", "mjs": "JavaScript", "cjs": "JavaScript",
    "ts": "TypeScript", "tsx": "TypeScript", "go": "Go", "rs": "Rust", "java": "Java", "kt": "Kotlin",
    "swift": "Swift", "rb": "Ruby", "php": "PHP", "cs": "C#", "c": "C", "h": "C", "cc": "C++", "cpp": "C++",
    "hpp": "C++", "dart": "Dart", "scala": "Scala", "sh": "Shell", "bash": "Shell", "zsh": "Shell",
    "html": "HTML", "css": "CSS", "scss": "CSS", "less": "CSS", "vue": "Vue", "svelte": "Svelte",
}
JS_FRAMEWORKS = {
    "react": "react", "next": "next", "vue": "vue", "nuxt": "nuxt", "svelte": "svelte", "@angular/core": "angular",
    "express": "express", "fastify": "fastify", "@nestjs/core": "nestjs", "electron": "electron", "vite": "vite",
    "tailwindcss": "tailwind", "koa": "koa", "hono": "hono",
}
JS_TEST_RUNNERS = {
    "jest": "jest", "vitest": "vitest", "mocha": "mocha", "ava": "ava", "@playwright/test": "playwright",
    "cypress": "cypress",
}
TEXT_FRAMEWORKS: dict[str, dict[str, str]] = {
    "python": {"django": "django", "flask": "flask", "fastapi": "fastapi", "textual": "textual",
               "sqlalchemy": "sqlalchemy", "typer": "typer", "pydantic": "pydantic", "celery": "celery"},
    "composer": {"laravel/framework": "laravel", "symfony/framework-bundle": "symfony", "symfony/symfony": "symfony"},
    "bundler": {"rails": "rails", "sinatra": "sinatra"},
    "cargo": {"axum": "axum", "actix-web": "actix-web", "tokio": "tokio", "rocket": "rocket"},
    "go": {"gin-gonic/gin": "gin", "labstack/echo": "echo", "gofiber/fiber": "fiber"},
    "pub": {"flutter": "flutter"},
    "maven": {"spring-boot": "spring"}, "gradle": {"spring-boot": "spring"},
}
CI_RE = re.compile(
    r"^(\.github/workflows/[^/]+\.ya?ml|\.gitlab-ci\.yml|\.circleci/config\.yml|azure-pipelines\.yml|"
    r"Jenkinsfile|bitbucket-pipelines\.yml|\.travis\.yml)$",
)
MAX_MANIFESTS = 20
MAX_MANIFEST_DEPTH = 3


def read_manifest(repo: Repo, rel: str, warnings: list[str]) -> str | None:
    text = read_candidate(repo, rel)
    if text is None:
        warnings.append(f"{rel}: unreadable or too large")
    return text


def js_manifest(rel: str, text: str, warnings: list[str]) -> tuple[list[str], list[dict[str, str | None]]]:
    try:
        data = json.loads(text)
    except ValueError:
        warnings.append(f"{rel}: invalid JSON")
        return [], []
    if not isinstance(data, dict):
        return [], []
    names: set[str] = set()
    for section in ("dependencies", "devDependencies", "peerDependencies"):
        deps = data.get(section)
        if isinstance(deps, dict):
            names.update(str(n) for n in deps)
    raw_scripts = data.get("scripts")
    scripts: dict[str, Any] = raw_scripts if isinstance(raw_scripts, dict) else {}
    test_script = scripts.get("test") if isinstance(scripts.get("test"), str) else None
    runners: list[dict[str, str | None]] = []
    for dependency, label in JS_TEST_RUNNERS.items():
        if dependency in names:
            command = test_script if test_script and label in test_script else None
            runners.append({"name": label, "command": command, "source": rel})
    if test_script and not any(r["command"] == test_script for r in runners):
        runners.append({"name": "npm test", "command": test_script, "source": rel})
    return sorted(JS_FRAMEWORKS[n] for n in names if n in JS_FRAMEWORKS), runners


def stack_repo(repo: Repo) -> dict[str, Any]:
    warnings: list[str] = []
    files = [rel for rel in tracked_files(repo) if not any(p in IGNORED_DIRS for p in rel.split("/")[:-1])]
    names = {rel.rsplit("/", 1)[-1] for rel in files}
    manifests = [
        {"path": rel, "kind": MANIFEST_KINDS[rel.rsplit("/", 1)[-1]]}
        for rel in files
        if rel.rsplit("/", 1)[-1] in MANIFEST_KINDS and rel.count("/") <= MAX_MANIFEST_DEPTH
    ][:MAX_MANIFESTS]
    counts: dict[str, int] = {}
    for rel in files:
        name = rel.rsplit("/", 1)[-1]
        if GENERATED_RE.search(name) or "." not in name:
            continue
        language = LANGUAGES.get(name.rsplit(".", 1)[-1].lower())
        if language:
            counts[language] = counts.get(language, 0) + 1
    languages = [{"name": n, "files": c} for n, c in sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))][:8]
    managers = sorted({LOCK_MANAGERS[n] for n in names if n in LOCK_MANAGERS})
    frameworks: set[str] = set()
    runners: list[dict[str, str | None]] = []
    for manifest in manifests:
        rel, kind = manifest["path"], manifest["kind"]
        text = read_manifest(repo, rel, warnings)
        if text is None:
            continue
        if kind == "npm":
            found, js_runners = js_manifest(rel, text, warnings)
            frameworks.update(found)
            runners.extend(js_runners)
            continue
        lowered = text.lower()
        for needle, label in TEXT_FRAMEWORKS.get(kind, {}).items():
            if needle in lowered:
                frameworks.add(label)
        runner = text_runner(kind, rel, lowered, names, managers)
        if runner:
            runners.append(runner)
    if "requirements.txt" in names and not any(m in managers for m in ("uv", "poetry", "pipenv")):
        managers.append("pip")
    if "package.json" in names and not any(m in managers for m in ("npm", "yarn", "pnpm", "bun")):
        managers.append("npm")
    return {
        "detected": bool(manifests or languages), "manifests": manifests, "languages": languages,
        "frameworks": sorted(frameworks), "package_managers": sorted(managers), "test_runners": runners,
        "ci": sorted(rel for rel in files if CI_RE.match(rel)), "warnings": warnings,
    }


def text_runner(
    kind: str, rel: str, lowered: str, names: set[str], managers: list[str],
) -> dict[str, str | None] | None:
    if kind == "python" and ("pytest" in lowered or {"pytest.ini", "conftest.py"} & names):
        return {"name": "pytest", "command": "uv run pytest" if "uv" in managers else "pytest", "source": rel}
    if kind == "go":
        return {"name": "go test", "command": "go test ./...", "source": rel}
    if kind == "cargo":
        return {"name": "cargo test", "command": "cargo test", "source": rel}
    if kind == "composer" and ("phpunit" in lowered or {"phpunit.xml", "phpunit.xml.dist"} & names):
        return {"name": "phpunit", "command": "vendor/bin/phpunit", "source": rel}
    if kind == "bundler" and "rspec" in lowered:
        return {"name": "rspec", "command": "bundle exec rspec", "source": rel}
    if kind == "pub":
        return {"name": "flutter test" if "flutter" in lowered else "dart test",
                "command": "flutter test" if "flutter" in lowered else "dart test", "source": rel}
    if kind == "maven":
        return {"name": "maven", "command": "mvn test", "source": rel}
    if kind == "gradle":
        return {"name": "gradle", "command": "./gradlew test" if "gradlew" in names else "gradle test", "source": rel}
    return None


def stack(args: argparse.Namespace) -> None:
    start = Path(args.root) if args.root else Path.cwd()
    if not start.is_dir():
        fail("--root must be a directory")
    emit({"repos": [
        {"name": r.name, "linked": r.linked, "label": r.label, **stack_repo(r)} for r in discover_repos(start)
    ]})


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Locate code for /explain and /feasibility")
    parser.add_argument("--root", help="directory inside the repo to search from (default: cwd)")
    sub = parser.add_subparsers(dest="command", required=True)
    find_parser = sub.add_parser("find")
    find_parser.add_argument("terms", nargs="+")
    find_parser.add_argument("--limit", type=int, default=DEFAULT_LIMIT)
    find_parser.add_argument("--offset", type=int, default=0)
    find_parser.set_defaults(func=find)
    sub.add_parser("stack").set_defaults(func=stack)
    return parser


def main() -> None:
    args = build_parser().parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
