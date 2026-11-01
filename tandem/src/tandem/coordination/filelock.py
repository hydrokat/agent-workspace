import sys
import time
from collections.abc import Iterator
from contextlib import contextmanager
from typing import Any


@contextmanager
def exclusive_lock(file: Any) -> Iterator[None]:
    fd = file.fileno() if hasattr(file, "fileno") else file
    if sys.platform == "win32":
        import msvcrt

        while True:
            try:
                if hasattr(file, "seek"):
                    file.seek(0)
                msvcrt.locking(fd, msvcrt.LK_LOCK, 1)
                break
            except OSError:
                time.sleep(0.05)
        try:
            yield
        finally:
            if hasattr(file, "seek"):
                file.seek(0)
            msvcrt.locking(fd, msvcrt.LK_UNLCK, 1)
    else:
        import fcntl

        fcntl.flock(fd, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(fd, fcntl.LOCK_UN)
