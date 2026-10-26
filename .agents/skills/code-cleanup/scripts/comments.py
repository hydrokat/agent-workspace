#!/usr/bin/env python3
"""Find and remove unnecessary source comments in one git repository.

Stdlib only. JSON on stdout. Exit codes: 0 ok, 1 drift/protected branch/refused, 2 invalid input.

Commands:
  scan [paths...] [--diff BASE] [--include-other] [--out FILE] [--limit N] [--offset N]
      {"repo", "files_scanned", "skipped": {"unsupported": {ext: n}, "ignored": n, "unreadable": n},
       "kept": {reason: n}, "counts": {kind: n}, "total", "offset", "limit", "next_offset",
       "candidates": [{id, file, start, end, kind, reason, text, trailing, col, source}]}
      Scans tracked files. Candidate kinds: empty, divider, commented-out-code, stale-todo,
      narration, restates-code, other. `other` is listed only with --include-other.
      Documentation-generator comments, directives, license headers, shebangs, and tracked TODOs
      are kept and never listed. --out writes the full unpaged result for `remove`.
  remove --plan FILE [--allow-protected-branch]
      The plan is JSON: {"scan": "<scan --out file>", "ids": [...], "kinds": [...], "files": [...],
      "all": false, "exclude": [...]}. A candidate is selected when its id, kind, or file is named
      (or all is true) and its id is not excluded. Every target is re-read and compared with the scan;
      any drift in a file leaves that file untouched and exits 1.
      {"removed", "files": [{file, removed, status}]}
"""
from __future__ import annotations

import argparse
import ast
import bisect
import io
import json
import os
import re
import subprocess
import sys
import tempfile
import textwrap
import tokenize
from dataclasses import dataclass
from pathlib import Path
from typing import Any, NoReturn

MAX_FILE_BYTES = 1_000_000
MAX_PLAN_BYTES = 20_000_000
DEFAULT_LIMIT = 50
MAX_LIMIT = 500
PROTECTED_BRANCHES = {"main", "master", "trunk"}
KINDS = ("empty", "divider", "commented-out-code", "stale-todo", "narration", "restates-code", "other")
BASE_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/~^@-]{0,99}$")

IGNORED_DIRS = {
    "node_modules", "vendor", ".venv", "venv", "dist", "build", ".claude", ".gemini", ".git",
    "__pycache__", ".next", "target", "coverage", ".tox",
}
LOCKFILES = {
    "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "poetry.lock", "uv.lock", "Cargo.lock",
    "Gemfile.lock", "composer.lock", "go.sum", "bun.lockb",
}
GENERATED_RE = re.compile(r"\.(min\.(js|css)|map|lock)$|\.d\.ts$", re.IGNORECASE)

DIRECTIVE_RE = re.compile(
    r"eslint-(disable|enable|env)|@ts-(ignore|expect-error|nocheck|check)|noqa|type:\s*ignore|"
    r"pragma|fmt:\s*(off|on|skip)|prettier-ignore|istanbul ignore|c8 ignore|nolint|rubocop:|"
    r"shellcheck|pylint:|pyright:|mypy:|go:(generate|build|embed)|\+build|@jsx|@flow|"
    r"sourceMappingURL|coding[:=]|-\*-|vim?:|yamllint|codecov|tslint:|jshint|stylelint-|"
    r"@generated|phpcs:|@phpstan|@psalm|NOSONAR|cspell:|webpack[A-Z]\w+|syntax=|escape=|"
    r"<reference|#region|#endregion|SPDX-License-Identifier",
    re.IGNORECASE,
)
LICENSE_RE = re.compile(r"copyright|licen[sc]e|all rights reserved", re.IGNORECASE)
SWAGGER_RE = re.compile(r"@OA\\|@swagger|@openapi|\bswagger:|\bopenapi:", re.IGNORECASE)
TODO_RE = re.compile(r"\b(TODO|FIXME|XXX|HACK)\b")
TRACKED_RE = re.compile(r"\b(TODO|FIXME|XXX|HACK)\s*[(\[@]|\b[A-Z][A-Z0-9]+-\d+\b|#\d+|https?://|issues?/\d+")
NARRATION_RE = re.compile(
    r"^(step\s*\d+|first(ly)?\b|next\b|then\b|now\b|finally\b|let'?s\b|let me\b|we (need|should|will|can)\b|"
    r"i (think|need|will|'ll|guess)\b|hmm|okay\b|ok\b|going to\b|trying\b|maybe\b|probably\b|"
    r"here we\b|at this point|so (we|now)\b)",
    re.IGNORECASE,
)
CODE_START_RE = re.compile(
    r"^(if|for|while|return|import|from|const|let|var|def|class|function|print|console\.|self\.|this\.|"
    r"await|raise|throw|else|elif|try|except|catch|finally|switch|case|break|continue|export|public|"
    r"private|static|fmt\.|System\.)\b"
)
CODE_PUNCT_RE = re.compile(r"[(){}\[\];=]")
ASSIGN_RE = re.compile(r"^[A-Za-z_$][\w$.\[\]]*\s*(=|\+=|-=|:=)\s*\S")
CALL_RE = re.compile(r"^[\w$.]+\(.*\)\s*;?$")
DIVIDER_RE = re.compile(r"^[\W_]{3,}$|^[\W_]{2,}\s*\w[\w\s]*\s*[\W_]{2,}$")
STOP_WORDS = {"the", "a", "an", "to", "of", "for", "and", "is", "it", "this", "that", "in", "on", "with", "we"}
GENERIC_VERBS = {
    "get", "set", "return", "returns", "create", "add", "update", "delete", "remove", "check", "call",
    "init", "initialize", "define", "declare", "increment", "decrement", "loop", "iterate", "print",
    "import", "assign", "compute", "calculate", "start", "end", "begin", "close", "open", "read", "write",
}
SCHEME_BEFORE_RE = re.compile(r"\b(https?|ftps?|wss?|file|ssh|git|s3|data):$", re.IGNORECASE)
EXPORTED_DECL_RE = re.compile(r"^(package\b|func\s+(\([^)]*\)\s*)?[A-Z]|(type|var|const)\s+[A-Z])")
HEREDOC_RE = re.compile(r"<<([-~]?)\s*(['\"]?)([A-Za-z_]\w*)\2")
YAML_BLOCK_RE = re.compile(r"(^|[\s:-])[|>][+-]?\d?\s*$")


@dataclass(frozen=True)
class Lang:
    family: str
    nested_block: bool = False
    backtick: str = ""
    regex: bool = False
    triple: bool = False
    hash_line: bool = False
    slash_line: bool = True
    doc_slash: bool = False
    rust: bool = False
    heredoc: str = ""
    css_url: bool = False
    multiline_strings: bool = False


C_STYLE = Lang("c")
LANGS: dict[str, Lang] = {
    "py": Lang("python"), "pyi": Lang("python"),
    "js": Lang("c", backtick="template", regex=True), "jsx": Lang("c", backtick="template", regex=True),
    "mjs": Lang("c", backtick="template", regex=True), "cjs": Lang("c", backtick="template", regex=True),
    "ts": Lang("c", backtick="template", regex=True), "tsx": Lang("c", backtick="template", regex=True),
    "java": Lang("c", triple=True), "kt": Lang("c", nested_block=True, triple=True),
    "kts": Lang("c", nested_block=True, triple=True), "swift": Lang("c", nested_block=True, triple=True, doc_slash=True),
    "scala": Lang("c", nested_block=True, triple=True),
    "go": Lang("c", backtick="raw"), "rs": Lang("c", nested_block=True, rust=True, doc_slash=True, multiline_strings=True),
    "c": C_STYLE, "h": C_STYLE, "cc": C_STYLE, "cpp": C_STYLE, "cxx": C_STYLE, "hpp": C_STYLE,
    "cs": Lang("c", triple=True, doc_slash=True), "dart": Lang("c", triple=True, doc_slash=True),
    "php": Lang("c", hash_line=True, heredoc="php", multiline_strings=True),
    "css": Lang("c", slash_line=False, css_url=True), "scss": Lang("c", css_url=True),
    "less": Lang("c", css_url=True),
    "sh": Lang("hash", heredoc="sh"), "bash": Lang("hash", heredoc="sh"), "zsh": Lang("hash", heredoc="sh"),
    "rb": Lang("hash", heredoc="rb"), "rake": Lang("hash", heredoc="rb"),
    "yml": Lang("hash"), "yaml": Lang("hash"), "toml": Lang("hash", triple=True),
}
NAMED_LANGS = {"Dockerfile": Lang("hash"), "Makefile": Lang("hash")}


def fail(message: str, code: int = 2, **extra: Any) -> NoReturn:
    print(json.dumps({"error": message, **extra}))
    sys.exit(code)


def emit(data: dict[str, Any]) -> None:
    print(json.dumps(data, ensure_ascii=False))


@dataclass
class Raw:
    kind: str
    line: int
    col: int
    end_line: int
    end_col: int


class Offsets:
    def __init__(self, text: str) -> None:
        self.starts = [0]
        for match in re.finditer("\n", text):
            self.starts.append(match.end())

    def at(self, offset: int) -> tuple[int, int]:
        index = bisect.bisect_right(self.starts, offset) - 1
        return index + 1, offset - self.starts[index]


def git(root: Path, *args: str) -> str:
    try:
        proc = subprocess.run(
            ["git", "-C", str(root), *args], capture_output=True, text=True, timeout=60, check=True,
        )
    except (OSError, subprocess.SubprocessError) as exc:
        detail = getattr(exc, "stderr", "") or str(exc)
        fail(f"git {args[0]} failed: {detail.strip()[:200]}", 1)
    return proc.stdout


def repo_root(start: str | None) -> Path:
    base = Path(start) if start else Path.cwd()
    if not base.is_dir():
        fail("--root must be a directory")
    try:
        out = subprocess.run(
            ["git", "-C", str(base), "rev-parse", "--show-toplevel"],
            capture_output=True, text=True, timeout=10, check=True,
        ).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        fail("not inside a git repository", 1)
    return Path(out).resolve()


def language_for(rel: str) -> Lang | None:
    name = rel.rsplit("/", 1)[-1]
    if name.endswith(".blade.php"):
        return None
    if name in NAMED_LANGS:
        return NAMED_LANGS[name]
    ext = name.rsplit(".", 1)[-1].lower() if "." in name else ""
    return LANGS.get(ext)


def ignored(rel: str) -> bool:
    parts = rel.split("/")
    return any(p in IGNORED_DIRS for p in parts[:-1]) or parts[-1] in LOCKFILES or bool(GENERATED_RE.search(parts[-1]))


def scan_python(text: str) -> list[Raw] | None:
    found: list[Raw] = []
    try:
        for token in tokenize.generate_tokens(io.StringIO(text).readline):
            if token.type == tokenize.COMMENT:
                row, col = token.start
                found.append(Raw("line", row, col, row, token.end[1]))
    except (tokenize.TokenError, IndentationError, SyntaxError):
        return None
    return found


def scan_hash(text: str, lang: Lang, ext_yaml_like: bool) -> list[Raw]:
    found: list[Raw] = []
    quote = ""
    heredoc_end: tuple[str, bool] | None = None
    block_indent: int | None = None
    for number, line in enumerate(text.split("\n"), 1):
        stripped = line.rstrip("\r")
        indent = len(stripped) - len(stripped.lstrip())
        if heredoc_end is not None:
            terminator, loose = heredoc_end
            if (stripped.strip() if loose else stripped) == terminator:
                heredoc_end = None
            continue
        if block_indent is not None:
            if not stripped.strip() or indent > block_indent:
                continue
            block_indent = None
        i, n, hit = 0, len(stripped), -1
        while i < n:
            c = stripped[i]
            if quote:
                if quote in ('"""', "'''"):
                    if stripped.startswith(quote, i):
                        i += 3
                        quote = ""
                        continue
                elif c == "\\" and quote == '"':
                    i += 2
                    continue
                elif c == quote:
                    quote = ""
                i += 1
                continue
            if lang.triple and stripped.startswith(('"""', "'''"), i):
                quote = stripped[i:i + 3]
                i += 3
                continue
            if c in "\"'" and (not ext_yaml_like or quote_may_open(stripped, i)):
                quote = c
                i += 1
                continue
            if c == "\\":
                i += 2
                continue
            if c == "#" and (i == 0 or stripped[i - 1] in " \t"):
                hit = i
                break
            i += 1
        code = stripped[:hit] if hit >= 0 else stripped
        if hit >= 0:
            found.append(Raw("line", number, hit, number, len(stripped)))
        if not quote:
            if lang.heredoc in ("sh", "rb"):
                match = HEREDOC_RE.search(code)
                if match and (lang.heredoc == "sh" or not code[match.start() - 1:match.start()].isalnum()):
                    heredoc_end = (match.group(3), match.group(1) in ("-", "~"))
            if ext_yaml_like and YAML_BLOCK_RE.search(code):
                block_indent = indent
    return found


def quote_may_open(line: str, index: int) -> bool:
    before = line[:index].rstrip()
    return not before or before[-1] in ":-,[{=?"


def regex_allowed(text: str, i: int) -> bool:
    before = text[max(0, i - 12):i].rstrip()
    if not before:
        return True
    if before[-1] in "(,=:[!&|?{};+-*%<>~^":
        return True
    return bool(re.search(r"\b(return|typeof|case|in|of|delete|void|throw|new|else|do|yield|await)$", before))


def skip_quoted(text: str, i: int, quote: str, multiline_ok: bool) -> int:
    n = len(text)
    i += 1
    while i < n:
        c = text[i]
        if c == "\\":
            i += 2
            continue
        if c == quote:
            return i + 1
        if c == "\n" and not multiline_ok:
            return i
        i += 1
    return n


def skip_regex(text: str, i: int) -> int | None:
    n, j, in_class = len(text), i + 1, False
    while j < n:
        c = text[j]
        if c == "\\":
            j += 2
            continue
        if c == "\n":
            return None
        if c == "[":
            in_class = True
        elif c == "]":
            in_class = False
        elif c == "/" and not in_class:
            j += 1
            while j < n and text[j].isalpha():
                j += 1
            return j
        j += 1
    return None


PHP_OPEN_RE = re.compile(r"<\?(?:php\b|=)?")
RUST_CHAR_RE = re.compile(r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]+\}|.)|[^\\'])'")
RUST_RAW_RE = re.compile(r'b?r(#*)"')


def scan_c(text: str, lang: Lang) -> list[Raw]:
    offsets = Offsets(text)
    found: list[Raw] = []
    n, i, brace = len(text), 0, 0
    template_stack: list[int] = []
    php = lang.heredoc == "php"
    in_html = php

    def template_body(start: int) -> int:
        nonlocal brace
        j = start
        while j < n:
            c = text[j]
            if c == "\\":
                j += 2
                continue
            if c == "`":
                return j + 1
            if c == "$" and text[j + 1:j + 2] == "{":
                template_stack.append(brace)
                brace += 1
                return j + 2
            j += 1
        return n

    def add(kind: str, start: int, end: int) -> None:
        line, col = offsets.at(start)
        end_line, end_col = offsets.at(end)
        found.append(Raw(kind, line, col, end_line, end_col))

    def line_end(start: int) -> int:
        end = text.find("\n", start)
        end = n if end == -1 else end
        if php:
            close = text.find("?>", start, end)
            if close != -1:
                return close
        return end - 1 if text[end - 1:end] == "\r" else end

    while i < n:
        c, nxt = text[i], text[i + 1:i + 2]
        if in_html:
            opener = PHP_OPEN_RE.search(text, i)
            if opener is None:
                break
            i, in_html = opener.end(), False
        elif php and c == "?" and nxt == ">":
            i, in_html = i + 2, True
        elif c == "/" and nxt == "/" and lang.slash_line and not SCHEME_BEFORE_RE.search(text[max(0, i - 8):i]):
            end = line_end(i)
            add("line", i, end)
            i = end
        elif c == "#" and lang.hash_line and nxt != "[":
            end = line_end(i)
            add("line", i, end)
            i = end
        elif c == "/" and nxt == "*":
            depth, j = 1, i + 2
            while j < n and depth:
                if lang.nested_block and text.startswith("/*", j):
                    depth += 1
                    j += 2
                elif text.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            add("block", i, j)
            i = j
        elif lang.rust and (m := RUST_RAW_RE.match(text, i)) and (i == 0 or not text[i - 1].isalnum()):
            closing = '"' + m.group(1)
            end = text.find(closing, m.end())
            i = n if end == -1 else end + len(closing)
        elif c == "<" and lang.heredoc == "php" and text.startswith("<<<", i):
            opener = re.compile(r"<<<\s*['\"]?(\w+)['\"]?\r?\n").match(text, i)
            if opener:
                closer = re.compile(rf"^\s*{re.escape(opener.group(1))}\b", re.MULTILINE).search(text, opener.end())
                i = closer.end() if closer else n
            else:
                i += 3
        elif lang.triple and text.startswith('"""', i):
            end = text.find('"""', i + 3)
            i = n if end == -1 else end + 3
        elif c == '"':
            i = skip_quoted(text, i, '"', lang.multiline_strings)
        elif c == "'":
            if lang.rust:
                m = RUST_CHAR_RE.match(text, i)
                i = m.end() if m else i + 1
            else:
                i = skip_quoted(text, i, "'", lang.multiline_strings)
        elif c == "`" and lang.backtick == "raw":
            end = text.find("`", i + 1)
            i = n if end == -1 else end + 1
        elif c == "`" and lang.backtick == "template":
            i = template_body(i + 1)
        elif c == "/" and lang.regex and regex_allowed(text, i):
            regex_end = skip_regex(text, i)
            i = regex_end if regex_end else i + 1
        elif c == "{":
            brace += 1
            i += 1
        elif c == "}":
            brace -= 1
            i += 1
            if template_stack and brace == template_stack[-1]:
                template_stack.pop()
                i = template_body(i)
        elif c == "u" and lang.css_url and text.startswith("url(", i):
            m = re.compile(r"url\(\s*[^'\")\s]").match(text, i)
            end = text.find(")", i) if m else -1
            i = end + 1 if end != -1 else i + 4
        else:
            i += 1
    return found


@dataclass
class Unit:
    kind: str
    start: int
    end: int
    col: int
    trailing: bool
    raw_lines: list[str]
    body: str
    keep: str | None = None


def strip_markers(raw: str, kind: str) -> str:
    if kind == "block":
        raw = raw[2:-2] if raw.endswith("*/") else raw[2:]
        lines = [re.sub(r"^\s*\*+\s?", "", line).strip() for line in raw.split("\n")]
        return "\n".join(lines).strip()
    return re.sub(r"^(#+|//+[/!]?|--)\s?", "", raw.strip())


def make_units(raws: list[Raw], lines: list[str], lang: Lang) -> list[Unit]:
    units: list[Unit] = []
    for raw in raws:
        first, last = lines[raw.line - 1], lines[raw.end_line - 1]
        whole_start = first[:raw.col].strip() == ""
        tail_blank = last[raw.end_col:].strip() == ""
        if not tail_blank:
            continue
        if raw.line == raw.end_line:
            text = first[raw.col:raw.end_col]
        else:
            text = "\n".join([first[raw.col:], *lines[raw.line:raw.end_line - 1], last[:raw.end_col]])
        body = strip_markers(text, raw.kind)
        if whole_start:
            units.append(Unit(raw.kind, raw.line, raw.end_line, raw.col,
                              False, lines[raw.line - 1:raw.end_line], body))
        elif raw.line == raw.end_line:
            units.append(Unit(raw.kind, raw.line, raw.end_line, raw.col, True, [text], body))
    return units


def keep_reason(unit: Unit, lang: Lang) -> str | None:
    first = unit.raw_lines[0].lstrip()
    if unit.start == 1 and first.startswith("#!"):
        return "shebang"
    if DIRECTIVE_RE.search(unit.body) or (unit.start <= 2 and re.search(r"coding[:=]", unit.body)):
        return "directive"
    if lang.family == "python" and first.startswith("#:"):
        return "doc"
    if unit.kind == "block" and first.startswith(("/**", "/*!")):
        return "doc"
    if lang.doc_slash and first.startswith(("///", "//!")):
        return "doc"
    if lang.family == "c" and first.startswith("//!"):
        return "doc"
    if SWAGGER_RE.search(unit.body):
        return "doc"
    if LICENSE_RE.search(unit.body) and unit.start <= 30:
        return "license"
    if TODO_RE.search(unit.body) and TRACKED_RE.search(unit.body):
        return "tracked-todo"
    return None


CODE_NODES = (
    ast.Assign, ast.AugAssign, ast.Import, ast.ImportFrom, ast.If, ast.For, ast.While, ast.Return,
    ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef, ast.With, ast.Try, ast.Raise, ast.Delete, ast.Assert,
)


def looks_like_code(body: str, lang: Lang) -> bool:
    lines = [line.strip() for line in body.split("\n") if line.strip()]
    if not lines:
        return False
    if lang.family == "python":
        try:
            tree = ast.parse(textwrap.dedent(body))
        except (SyntaxError, ValueError):
            tree = None
        if tree is not None:
            return any(is_code_node(node) for node in tree.body)
    hits = sum(1 for line in lines if line_is_code(line))
    return hits * 10 >= len(lines) * 7


def is_code_node(node: ast.stmt) -> bool:
    if isinstance(node, ast.AnnAssign):
        return node.value is not None
    if isinstance(node, ast.Expr):
        return isinstance(node.value, (ast.Call, ast.Await))
    return isinstance(node, CODE_NODES)


def line_is_code(line: str) -> bool:
    if line.endswith((";", "{", "}")) and CODE_PUNCT_RE.search(line):
        return True
    if CODE_START_RE.match(line) and CODE_PUNCT_RE.search(line) and not line.endswith((".", "?")):
        return True
    return bool(ASSIGN_RE.match(line) or CALL_RE.match(line))


def words_of(text: str) -> set[str]:
    spaced = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", text)
    return {w.lower() for w in re.findall(r"[A-Za-z]+", spaced)}


def classify(body: str, code_line: str | None, lang: Lang) -> str:
    flat = " ".join(body.split())
    if not re.search(r"\w", flat):
        return "empty" if not flat else "divider"
    if DIVIDER_RE.match(flat):
        return "divider"
    if looks_like_code(body, lang):
        return "commented-out-code"
    if TODO_RE.search(flat):
        return "stale-todo"
    if NARRATION_RE.match(flat):
        return "narration"
    if code_line is not None:
        comment_words = words_of(flat) - STOP_WORDS
        if 0 < len(comment_words) <= 6 and comment_words <= words_of(code_line) | GENERIC_VERBS:
            return "restates-code"
    return "other"


REASONS = {
    "empty": "comment with no text",
    "divider": "decorative divider",
    "commented-out-code": "disabled code left in a comment",
    "stale-todo": "TODO/FIXME with no owner or ticket",
    "narration": "narrates a step instead of documenting code",
    "restates-code": "words repeat the adjacent code",
    "other": "explanatory comment; review for a non-obvious why",
}


def next_code_line(lines: list[str], after: int, units: list[Unit]) -> str | None:
    commented = {n for u in units for n in range(u.start, u.end + 1) if not u.trailing}
    for number in range(after + 1, len(lines) + 1):
        if number in commented or not lines[number - 1].strip():
            if number in commented:
                return None
            continue
        return lines[number - 1]
    return None


def adjacent(previous: Unit, unit: Unit) -> bool:
    return (
        not previous.trailing and not unit.trailing and previous.kind == "line" and unit.kind == "line"
        and previous.end + 1 == unit.start and previous.col == unit.col
    )


def group_by_kind(run: list[Unit], kinds: list[str]) -> list[tuple[list[Unit], str]]:
    groups: list[tuple[list[Unit], str]] = []
    for unit, kind in zip(run, kinds):
        if groups and groups[-1][1] == kind and adjacent(groups[-1][0][-1], unit):
            groups[-1][0].append(unit)
        else:
            groups.append(([unit], kind))
    return groups


def code_line_for(unit: Unit, lines: list[str], units: list[Unit]) -> str | None:
    if unit.trailing:
        prefix = lines[unit.start - 1][:unit.col]
        return prefix if prefix.strip() else None
    return next_code_line(lines, unit.end, units)


def go_documented(units: list[Unit], lines: list[str]) -> set[int]:
    documented: set[int] = set()
    follows_declaration = False
    for index in range(len(units) - 1, -1, -1):
        unit = units[index]
        nxt = units[index + 1] if index + 1 < len(units) else None
        if nxt is not None and not unit.trailing and not nxt.trailing and nxt.start == unit.end + 1:
            pass
        else:
            following = lines[unit.end] if unit.end < len(lines) else ""
            follows_declaration = not unit.trailing and bool(EXPORTED_DECL_RE.match(following))
        if follows_declaration:
            documented.add(id(unit))
    return documented


def extract(text: str, lang: Lang, ext_yaml_like: bool) -> tuple[list[dict[str, Any]], dict[str, int]] | None:
    lines = text.split("\n")
    lines = [line[:-1] if line.endswith("\r") else line for line in lines]
    if lang.family == "python":
        raws = scan_python(text)
    elif lang.family == "hash":
        raws = scan_hash(text, lang, ext_yaml_like)
    else:
        raws = scan_c(text, lang)
    if raws is None:
        return None
    raws.sort(key=lambda r: (r.line, r.col))
    units = make_units(raws, lines, lang)
    kept: dict[str, int] = {}
    candidates: list[dict[str, Any]] = []
    documented = go_documented(units, lines) if lang.backtick == "raw" else set()
    for unit in units:
        unit.keep = keep_reason(unit, lang) or ("doc" if id(unit) in documented else None)
        if unit.keep:
            kept[unit.keep] = kept.get(unit.keep, 0) + 1
    runs: list[list[Unit]] = []
    for unit in units:
        if unit.keep:
            continue
        if runs and adjacent(runs[-1][-1], unit):
            runs[-1].append(unit)
        else:
            runs.append([unit])
    for run in runs:
        if len(run) > 1 and looks_like_code("\n".join(u.body for u in run), lang):
            groups = [(run, "commented-out-code")]
        else:
            groups = group_by_kind(run, [classify(u.body, code_line_for(u, lines, units), lang) for u in run])
        for members, kind in groups:
            first, last = members[0], members[-1]
            source = "\n".join(lines[first.start - 1:last.end])
            candidates.append({
                "start": first.start, "end": last.end, "kind": kind, "reason": REASONS[kind],
                "text": first.raw_lines[0] if first.trailing else textwrap.dedent(source),
                "trailing": first.trailing, "col": first.col, "source": source,
            })
    return candidates, kept


def changed_lines(root: Path, base: str) -> dict[str, set[int]]:
    if not BASE_RE.match(base):
        fail("invalid --diff base")
    try:
        subprocess.run(
            ["git", "-C", str(root), "rev-parse", "--verify", "--quiet", f"{base}^{{commit}}"],
            capture_output=True, timeout=10, check=True,
        )
    except (OSError, subprocess.SubprocessError):
        fail(f"unknown --diff base: {base}")
    result: dict[str, set[int]] = {}
    current = ""
    diff = git(root, "diff", "-U0", "--no-color", "--no-renames", base)
    for line in diff.splitlines():
        if line.startswith("+++ b/"):
            current = line[6:]
            result.setdefault(current, set())
        elif line.startswith("@@") and current:
            match = re.match(r"@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", line)
            if match:
                start, count = int(match.group(1)), int(match.group(2) or 1)
                result[current].update(range(start, start + count))
    return result


def resolve_paths(root: Path, raw_paths: list[str]) -> list[str]:
    prefixes = []
    for raw in raw_paths:
        if "\x00" in raw:
            fail("invalid path")
        candidate = (Path(raw) if Path(raw).is_absolute() else Path.cwd() / raw).resolve()
        if not candidate.is_relative_to(root):
            fail(f"path outside the repository: {raw}")
        prefixes.append("" if candidate == root else candidate.relative_to(root).as_posix())
    return prefixes


def cmd_scan(args: argparse.Namespace) -> None:
    root = repo_root(args.root)
    if not 1 <= args.limit <= MAX_LIMIT or args.offset < 0:
        fail(f"--limit must be 1..{MAX_LIMIT} and --offset >= 0")
    prefixes = resolve_paths(root, args.paths)
    tracked = [p for p in git(root, "ls-files", "-z").split("\0") if p]
    if prefixes:
        tracked = [p for p in tracked if any(x == "" or p == x or p.startswith(x + "/") for x in prefixes)]
    only_lines = changed_lines(root, args.diff) if args.diff else None
    skipped: dict[str, Any] = {"unsupported": {}, "ignored": 0, "unreadable": 0}
    kept_total: dict[str, int] = {}
    everything: list[dict[str, Any]] = []
    scanned = 0
    for rel in tracked:
        if only_lines is not None and rel not in only_lines:
            continue
        if ignored(rel):
            skipped["ignored"] += 1
            continue
        lang = language_for(rel)
        if lang is None:
            ext = Path(rel).suffix.lower() or Path(rel).name
            skipped["unsupported"][ext] = skipped["unsupported"].get(ext, 0) + 1
            continue
        path = root / rel
        try:
            if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_FILE_BYTES:
                raise OSError
            raw_bytes = path.read_bytes()
            if b"\x00" in raw_bytes[:8192]:
                raise OSError
            text = raw_bytes.decode("utf-8")
        except (OSError, UnicodeDecodeError):
            skipped["unreadable"] += 1
            continue
        result = extract(text, lang, rel.lower().endswith((".yml", ".yaml", ".toml")))
        if result is None:
            skipped["unreadable"] += 1
            continue
        scanned += 1
        found, kept = result
        for reason, count in kept.items():
            kept_total[reason] = kept_total.get(reason, 0) + count
        for item in found:
            if only_lines is not None and not set(range(item["start"], item["end"] + 1)) & only_lines[rel]:
                continue
            everything.append({"file": rel, **item})
    counts = {kind: 0 for kind in KINDS}
    for item in everything:
        counts[item["kind"]] += 1
    listed = [c for c in everything if args.include_other or c["kind"] != "other"]
    for index, item in enumerate(listed, 1):
        item["id"] = f"c{index}"
    full: dict[str, Any] = {
        "repo": root.name, "files_scanned": scanned, "skipped": skipped, "kept": kept_total,
        "counts": counts, "total": len(listed), "candidates": [ordered(c) for c in listed],
    }
    if args.out:
        out_path = Path(args.out)
        write_atomic(out_path, json.dumps(full, ensure_ascii=False, indent=1))
    page = full["candidates"][args.offset:args.offset + args.limit]
    next_offset = args.offset + args.limit if args.offset + args.limit < len(listed) else None
    emit({**full, "candidates": page, "offset": args.offset, "limit": args.limit, "next_offset": next_offset})


def ordered(item: dict[str, Any]) -> dict[str, Any]:
    keys = ("id", "file", "start", "end", "kind", "reason", "text", "trailing", "col", "source")
    return {k: item[k] for k in keys}


def write_atomic(path: Path, content: str, mode: int | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=str(path.parent), prefix=".comments-", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as handle:
            handle.write(content)
        if mode is not None:
            os.chmod(tmp, mode)
        os.replace(tmp, path)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise


def load_json_file(path_arg: str, label: str) -> dict[str, Any]:
    path = Path(path_arg)
    try:
        if path.stat().st_size > MAX_PLAN_BYTES:
            fail(f"{label} is larger than {MAX_PLAN_BYTES} bytes")
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        fail(f"cannot read {label}: {path_arg}")
    if not isinstance(data, dict):
        fail(f"{label} must be a JSON object")
    return data


def select(plan: dict[str, Any], scan: dict[str, Any]) -> list[dict[str, Any]]:
    candidates = scan.get("candidates")
    if not isinstance(candidates, list):
        fail("scan file has no candidates")
    ids, kinds, files = (set(plan.get(k) or []) for k in ("ids", "kinds", "files"))
    exclude = set(plan.get("exclude") or [])
    if not (ids or kinds or files or plan.get("all") is True):
        fail("plan selects nothing: name ids, kinds, files, or set all")
    unknown = kinds - set(KINDS)
    if unknown:
        fail(f"unknown kinds: {sorted(unknown)}")
    chosen = []
    for item in candidates:
        if not isinstance(item, dict) or item.get("id") in exclude:
            continue
        if plan.get("all") is True or item.get("id") in ids or item.get("kind") in kinds or item.get("file") in files:
            chosen.append(item)
    missing = ids - {i.get("id") for i in candidates if isinstance(i, dict)}
    if missing:
        fail(f"ids not in scan: {sorted(missing)}")
    return chosen


def valid_candidate(item: dict[str, Any]) -> bool:
    return (
        isinstance(item.get("file"), str) and isinstance(item.get("source"), str)
        and isinstance(item.get("text"), str) and isinstance(item.get("trailing"), bool)
        and all(isinstance(item.get(k), int) and item[k] >= 0 for k in ("start", "end", "col"))
        and item["start"] >= 1 and item["end"] >= item["start"]
    )


def apply_removals(text: str, items: list[dict[str, Any]]) -> str | None:
    pieces = text.split("\n")
    eols = ["\r" if p.endswith("\r") else "" for p in pieces]
    lines = [p[:-1] if p.endswith("\r") else p for p in pieces]
    for item in items:
        start, end = item["start"], item["end"]
        if end > len(lines) or "\n".join(lines[start - 1:end]) != item["source"]:
            return None
        if item["trailing"] and not lines[start - 1][item["col"]:].startswith(item["text"]):
            return None
    drop: set[int] = set()
    for item in sorted(items, key=lambda i: i["start"], reverse=True):
        start, end = item["start"], item["end"]
        if item["trailing"]:
            lines[start - 1] = lines[start - 1][:item["col"]].rstrip()
        else:
            drop.update(range(start, end + 1))
    kept = [(line, eol) for number, (line, eol) in enumerate(zip(lines, eols), 1) if number not in drop]
    collapsed: set[int] = set()
    for site in sorted({i["start"] for i in items if not i["trailing"]}):
        position = site - 1 - len([d for d in drop if d < site])
        if 0 < position < len(kept) and not kept[position - 1][0].strip() and not kept[position][0].strip():
            collapsed.add(position)
    return "\n".join(line + eol for index, (line, eol) in enumerate(kept) if index not in collapsed)


def python_parses(text: str) -> bool:
    try:
        ast.parse(text)
    except (SyntaxError, ValueError):
        return False
    return True


def cmd_remove(args: argparse.Namespace) -> None:
    root = repo_root(args.root)
    branch = git(root, "branch", "--show-current").strip()
    if branch in PROTECTED_BRANCHES and not args.allow_protected_branch:
        fail(f"refusing to edit files on protected branch {branch!r}", 1, branch=branch)
    plan = load_json_file(args.plan, "plan")
    scan_path = plan.get("scan")
    if not isinstance(scan_path, str):
        fail("plan.scan must be the path of a scan --out file")
    chosen = select(plan, load_json_file(scan_path, "scan file"))
    if not all(valid_candidate(c) for c in chosen):
        fail("scan file has malformed candidates")
    tracked = {p for p in git(root, "ls-files", "-z").split("\0") if p}
    by_file: dict[str, list[dict[str, Any]]] = {}
    for item in chosen:
        by_file.setdefault(item["file"], []).append(item)
    report: list[dict[str, Any]] = []
    refused = False
    for rel, items in sorted(by_file.items()):
        path = root / rel
        status = "removed"
        if rel not in tracked or path.is_symlink() or not path.is_file() or not path.resolve().is_relative_to(root):
            report.append({"file": rel, "removed": 0, "status": "refused: not a tracked regular file in the repository"})
            refused = True
            continue
        try:
            original = path.read_bytes().decode("utf-8")
        except (OSError, UnicodeDecodeError):
            report.append({"file": rel, "removed": 0, "status": "refused: unreadable"})
            refused = True
            continue
        updated = apply_removals(original, items)
        if updated is None:
            status = "drift: file changed since the scan"
        elif rel.endswith((".py", ".pyi")) and python_parses(original) and not python_parses(updated):
            status, updated = "refused: result would not parse", None
        if updated is None:
            report.append({"file": rel, "removed": 0, "status": status})
            refused = True
            continue
        write_atomic(path, updated, mode=path.stat().st_mode & 0o777)
        report.append({"file": rel, "removed": len(items), "status": status})
    emit({"removed": sum(r["removed"] for r in report), "files": report})
    if refused:
        sys.exit(1)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Find and remove unnecessary comments")
    parser.add_argument("--root", help="directory inside the repository (default: cwd)")
    sub = parser.add_subparsers(dest="command", required=True)
    scan = sub.add_parser("scan")
    scan.add_argument("paths", nargs="*")
    scan.add_argument("--diff")
    scan.add_argument("--include-other", action="store_true")
    scan.add_argument("--out")
    scan.add_argument("--limit", type=int, default=DEFAULT_LIMIT)
    scan.add_argument("--offset", type=int, default=0)
    scan.set_defaults(func=cmd_scan)
    remove = sub.add_parser("remove")
    remove.add_argument("--plan", required=True)
    remove.add_argument("--allow-protected-branch", action="store_true")
    remove.set_defaults(func=cmd_remove)
    return parser


def main() -> None:
    args = build_parser().parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
