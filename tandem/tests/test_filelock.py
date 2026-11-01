import multiprocessing
import sys
import time
from collections.abc import Generator
from pathlib import Path
from types import ModuleType
from typing import Any

import pytest
from tandem.coordination import filelock


def _holder_process(lock_path: Path, hold_seconds: float, started_file: Path) -> None:
    with open(lock_path, "a+") as f:
        with filelock.exclusive_lock(f):
            started_file.write_text("locked")
            time.sleep(hold_seconds)


@pytest.mark.skipif(sys.platform == "win32", reason="POSIX-specific locking test")
def test_exclusive_lock_posix_serializes_processes(tmp_path: Path) -> None:
    lock_path = tmp_path / "test.lock"
    started_file = tmp_path / "started.txt"

    proc = multiprocessing.Process(
        target=_holder_process, args=(lock_path, 0.4, started_file)
    )
    proc.start()

    for _ in range(50):
        if started_file.exists():
            break
        time.sleep(0.01)

    t0 = time.monotonic()
    with open(lock_path, "a+") as f:
        with filelock.exclusive_lock(f):
            elapsed = time.monotonic() - t0

    proc.join(timeout=2.0)
    assert elapsed >= 0.2


def test_windows_locking_branch(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    calls: list[tuple[int, int, int]] = []

    class FakeMsvcrt:
        LK_LOCK = 1
        LK_UNLCK = 2
        attempts = 0

        @classmethod
        def locking(cls, fd: int, mode: int, nbytes: int) -> None:
            calls.append((fd, mode, nbytes))
            if mode == cls.LK_LOCK and cls.attempts == 0:
                cls.attempts += 1
                raise OSError("busy")

    monkeypatch.setattr(sys, "platform", "win32")
    monkeypatch.setitem(sys.modules, "msvcrt", FakeMsvcrt)

    lock_file = tmp_path / "win.lock"
    with open(lock_file, "a+") as f:
        with filelock.exclusive_lock(f):
            assert len(calls) == 2
            assert calls[0][1] == FakeMsvcrt.LK_LOCK
            assert calls[1][1] == FakeMsvcrt.LK_LOCK

    assert len(calls) == 3
    assert calls[2][1] == FakeMsvcrt.LK_UNLCK
    assert calls[2][2] == 1
