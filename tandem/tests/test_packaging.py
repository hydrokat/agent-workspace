import hashlib
import importlib.util
import platform
import tarfile
import zipfile
from pathlib import Path
from typing import Any

import pytest


def _load_build_module() -> object:
    path = Path(__file__).resolve().parent.parent / "packaging" / "build.py"
    spec = importlib.util.spec_from_file_location("tandem_packaging_build", path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {path}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


build: Any = _load_build_module()


def test_platform_slug_normalisation() -> None:
    assert build.platform_slug("Darwin", "arm64") == ("darwin", "arm64")
    assert build.platform_slug("Darwin", "aarch64") == ("darwin", "arm64")
    assert build.platform_slug("Linux", "aarch64") == ("linux", "arm64")
    assert build.platform_slug("Linux", "x86_64") == ("linux", "x86_64")
    assert build.platform_slug("Windows", "AMD64") == ("windows", "x86_64")
    assert build.platform_slug("win32", "AMD64") == ("windows", "x86_64")
    assert build.platform_slug("Windows", "x86_64") == ("windows", "x86_64")
    assert build.platform_slug("Windows", "arm64") == ("windows", "arm64")


def test_asset_name() -> None:
    assert build.asset_name("1.5.0", "darwin", "arm64") == "tandem-1.5.0-darwin-arm64.tar.gz"
    assert build.asset_name("1.5.0", "linux", "x86_64") == "tandem-1.5.0-linux-x86_64.tar.gz"
    assert build.asset_name("1.5.0", "windows", "x86_64") == "tandem-1.5.0-windows-x86_64.zip"


def test_archive_tar_gz_top_level_layout(tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist" / "tandem"
    dist_dir.mkdir(parents=True)
    (dist_dir / "tandem").write_text("bin")
    (dist_dir / "lib.so").write_text("lib")

    out_dir = tmp_path / "out"
    tar_path = build.create_archive(dist_dir, out_dir, "1.5.0", "darwin", "arm64")

    assert tar_path.name == "tandem-1.5.0-darwin-arm64.tar.gz"
    assert tar_path.is_file()

    with tarfile.open(tar_path, "r:gz") as tar:
        names = tar.getnames()
        assert "tandem/tandem" in names
        assert "tandem/lib.so" in names
        assert all(n.startswith("tandem/") or n == "tandem" for n in names)


def test_archive_zip_top_level_layout(tmp_path: Path) -> None:
    dist_dir = tmp_path / "dist" / "tandem"
    dist_dir.mkdir(parents=True)
    (dist_dir / "tandem.exe").write_text("exe")
    (dist_dir / "lib.dll").write_text("dll")

    out_dir = tmp_path / "out"
    zip_path = build.create_archive(dist_dir, out_dir, "1.5.0", "windows", "x86_64")

    assert zip_path.name == "tandem-1.5.0-windows-x86_64.zip"
    assert zip_path.is_file()

    with zipfile.ZipFile(zip_path, "r") as zf:
        names = zf.namelist()
        assert "tandem/tandem.exe" in names
        assert "tandem/lib.dll" in names
        assert all(n.startswith("tandem/") or n == "tandem" for n in names)


def test_write_sha256(tmp_path: Path) -> None:
    archive_file = tmp_path / "test-archive.tar.gz"
    content = b"sample archive payload"
    archive_file.write_bytes(content)

    expected_hash = hashlib.sha256(content).hexdigest()
    sha_file = build.write_sha256(archive_file)

    assert sha_file == tmp_path / "test-archive.tar.gz.sha256"
    assert sha_file.is_file()

    line = sha_file.read_text().strip()
    assert line == f"{expected_hash}  test-archive.tar.gz"
