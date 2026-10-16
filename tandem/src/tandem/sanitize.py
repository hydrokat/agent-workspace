import re

_OSC = re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)?")
_CSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
_ESC = re.compile(r"\x1b[@-_]?")
_CTRL = re.compile(r"[\x00-\x08\x0b-\x1f\x7f-\x9f]")


def clean(text: str) -> str:
    """Remove terminal escape sequences and control characters from untrusted text (newlines kept)."""
    text = text.replace("\r\n", "\n")
    for pattern in (_OSC, _CSI, _ESC, _CTRL):
        text = pattern.sub("", text)
    return text
