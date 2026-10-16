import pytest

from tandem import __version__
from tandem.cli import main


def test_version_flag(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exc:
        main(["--version"])
    assert exc.value.code == 0
    assert __version__ in capsys.readouterr().out


def test_help_flag(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exc:
        main(["--help"])
    assert exc.value.code == 0
    assert "tandem" in capsys.readouterr().out


def test_unknown_flag_exits_nonzero() -> None:
    with pytest.raises(SystemExit) as exc:
        main(["--nope"])
    assert exc.value.code != 0


def test_importing_tandem_commands_does_not_load_hashlib() -> None:
    import subprocess
    import sys

    code = "import sys, tandem.cli, tandem.commands, tandem.status.app; sys.exit('hashlib' in sys.modules)"
    assert subprocess.run([sys.executable, "-c", code]).returncode == 0
