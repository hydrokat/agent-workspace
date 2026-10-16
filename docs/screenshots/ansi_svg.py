"""Render tmux ``capture-pane -e`` output (ANSI SGR text) as a self-contained SVG terminal screenshot."""
import re
from dataclasses import dataclass, replace
from xml.sax.saxutils import escape

PALETTE = ["#1e1e1e", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#abb2bf",
           "#5c6370", "#ff7b86", "#b5e890", "#f0d08a", "#7ec1ff", "#e08cf0", "#6fd3df", "#ffffff"]
DEFAULT_FG, DEFAULT_BG = "#d7dae0", "#1e1e1e"
CELL_W, CELL_H, PAD = 8.4, 18.0, 14
FONT = "SFMono-Regular, Menlo, Consolas, 'DejaVu Sans Mono', 'Liberation Mono', monospace"
_CSI = re.compile(r"\x1b\[([0-9;:]*)([A-Za-z])")


@dataclass(frozen=True)
class Style:
    fg: str | None = None
    bg: str | None = None
    bold: bool = False
    dim: bool = False
    reverse: bool = False


def _xterm256(n: int) -> str:
    if n < 16:
        return PALETTE[n]
    if n < 232:
        n -= 16
        level = [0, 95, 135, 175, 215, 255]
        return "#%02x%02x%02x" % (level[n // 36], level[(n // 6) % 6], level[n % 6])
    v = 8 + (n - 232) * 10
    return "#%02x%02x%02x" % (v, v, v)


def _apply(style: Style, params: str) -> Style:
    codes = [int(p) if p else 0 for p in params.replace(":", ";").split(";")] or [0]
    i = 0
    while i < len(codes):
        c = codes[i]
        if c == 0:
            style = Style()
        elif c == 1:
            style = replace(style, bold=True)
        elif c == 2:
            style = replace(style, dim=True)
        elif c == 7:
            style = replace(style, reverse=True)
        elif c in (22,):
            style = replace(style, bold=False, dim=False)
        elif c == 27:
            style = replace(style, reverse=False)
        elif 30 <= c <= 37:
            style = replace(style, fg=PALETTE[c - 30])
        elif 90 <= c <= 97:
            style = replace(style, fg=PALETTE[c - 90 + 8])
        elif 40 <= c <= 47:
            style = replace(style, bg=PALETTE[c - 40])
        elif 100 <= c <= 107:
            style = replace(style, bg=PALETTE[c - 100 + 8])
        elif c == 39:
            style = replace(style, fg=None)
        elif c == 49:
            style = replace(style, bg=None)
        elif c in (38, 48) and i + 1 < len(codes):
            if codes[i + 1] == 5 and i + 2 < len(codes):
                color, i = _xterm256(codes[i + 2]), i + 2
            elif codes[i + 1] == 2 and i + 4 < len(codes):
                color, i = "#%02x%02x%02x" % tuple(codes[i + 2:i + 5]), i + 4
            else:
                color = None
            if color:
                style = replace(style, fg=color) if c == 38 else replace(style, bg=color)
        i += 1
    return style


def parse(text: str) -> list[list[tuple[str, Style]]]:
    """Rows of (character, style) cells."""
    rows: list[list[tuple[str, Style]]] = []
    style = Style()
    for line in text.split("\n"):
        row: list[tuple[str, Style]] = []
        pos = 0
        for match in _CSI.finditer(line):
            row += [(ch, style) for ch in line[pos:match.start()]]
            if match.group(2) == "m":
                style = _apply(style, match.group(1))
            pos = match.end()
        row += [(ch, style) for ch in line[pos:]]
        rows.append(row)
    return rows


def render(text: str, cols: int, rows_count: int, title: str = "") -> str:
    grid = parse(text)[:rows_count]
    width = cols * CELL_W + 2 * PAD
    top = PAD + (22 if title else 0)
    height = rows_count * CELL_H + top + PAD
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0f}" height="{height:.0f}" '
           f'viewBox="0 0 {width:.0f} {height:.0f}" role="img" aria-label="{escape(title or "tandem screenshot")}">',
           f'<rect width="100%" height="100%" rx="10" fill="{DEFAULT_BG}"/>']
    if title:
        out.append(f'<text x="{PAD}" y="{PAD + 8}" font-family="{FONT}" font-size="12" fill="#7f848e">{escape(title)}</text>')
    backgrounds: list[str] = []
    texts: list[str] = []
    for r, row in enumerate(grid):
        y = top + r * CELL_H
        col = 0
        while col < len(row):
            ch, st = row[col]
            fg = st.fg or DEFAULT_FG
            bg = st.bg
            if st.reverse:
                fg, bg = (st.bg or DEFAULT_BG), (st.fg or DEFAULT_FG)
            end = col
            while end < len(row) and row[end][1] == st:
                end += 1
            if bg:
                backgrounds.append(f'<rect x="{PAD + col * CELL_W:.1f}" y="{y:.1f}" width="{(end - col) * CELL_W:.1f}" '
                                   f'height="{CELL_H:.1f}" fill="{bg}"/>')
            run = "".join(c for c, _ in row[col:end])
            if run.strip():
                xs = " ".join(f"{PAD + (col + k) * CELL_W:.1f}" for k in range(end - col))
                weight = ' font-weight="bold"' if st.bold else ""
                opacity = ' opacity="0.6"' if st.dim else ""
                texts.append(f'<text x="{xs}" y="{y + CELL_H - 5:.1f}" fill="{fg}"{weight}{opacity} xml:space="preserve">'
                             f'{escape(run)}</text>')
            col = end
    out.append(f'<g>{"".join(backgrounds)}</g>')
    out.append(f'<g font-family="{FONT}" font-size="13.5">{"".join(texts)}</g>')
    out.append("</svg>")
    return "\n".join(out)
