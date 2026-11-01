import hashlib
import os
import platform
import subprocess
import sys
import tarfile
import zipfile
from pathlib import Path


def platform_slug(system: str | None = None, machine: str | None = None) -> tuple[str, str]:
    raw_sys = (system or platform.system()).lower()
    raw_mach = (machine or platform.machine()).lower()

    if "darwin" in raw_sys or "mac" in raw_sys:
        os_slug = "darwin"
    elif "linux" in raw_sys:
        os_slug = "linux"
    elif "win" in raw_sys:
        os_slug = "windows"
    else:
        os_slug = raw_sys

    if raw_mach in ("arm64", "aarch64"):
        arch_slug = "arm64"
    elif raw_mach in ("x86_64", "amd64"):
        arch_slug = "x86_64"
    else:
        arch_slug = raw_mach

    return os_slug, arch_slug


def asset_name(version: str, os_name: str, arch: str) -> str:
    ext = "zip" if os_name == "windows" else "tar.gz"
    return f"tandem-{version}-{os_name}-{arch}.{ext}"


def create_archive(
    dist_dir: Path, output_dir: Path, version: str, os_name: str, arch: str
) -> Path:
    archive_file = output_dir / asset_name(version, os_name, arch)
    output_dir.mkdir(parents=True, exist_ok=True)

    if os_name == "windows":
        with zipfile.ZipFile(archive_file, "w", compression=zipfile.ZIP_DEFLATED) as zf:
            for root, _, files in os.walk(dist_dir):
                for f in files:
                    full_path = Path(root) / f
                    rel_path = full_path.relative_to(dist_dir)
                    zf.write(full_path, arcname=f"tandem/{rel_path.as_posix()}")
    else:
        with tarfile.open(archive_file, "w:gz") as tf:
            tf.add(dist_dir, arcname="tandem")

    return archive_file


def write_sha256(archive_path: Path) -> Path:
    hasher = hashlib.sha256()
    with open(archive_path, "rb") as f:
        while chunk := f.read(65536):
            hasher.update(chunk)
    digest = hasher.hexdigest()
    sha_path = archive_path.with_name(f"{archive_path.name}.sha256")
    sha_path.write_text(f"{digest}  {archive_path.name}\n")
    return sha_path


def run_build(spec_path: Path, dist_tandem_dir: Path, work_dir: Path, clean: bool = True) -> None:
    dist_parent = dist_tandem_dir.parent
    cmd = [
        sys.executable,
        "-m",
        "PyInstaller",
        "--noconfirm",
        *(["--clean"] if clean else []),
        f"--distpath={dist_parent}",
        f"--workpath={work_dir}",
        str(spec_path),
    ]
    subprocess.run(cmd, check=True)


def smoke_test(dist_tandem_dir: Path, expected_version: str) -> None:
    bin_name = "tandem.exe" if platform.system() == "Windows" else "tandem"
    bin_path = dist_tandem_dir / bin_name
    if not bin_path.is_file():
        raise FileNotFoundError(f"built binary not found: {bin_path}")

    env = {"PATH": "/usr/bin:/bin"} if platform.system() != "Windows" else dict(os.environ)

    res_version = subprocess.run(
        [str(bin_path), "--version"],
        env=env,
        capture_output=True,
        text=True,
        check=True,
    )
    if f"tandem {expected_version}" not in res_version.stdout:
        raise ValueError(f"unexpected version output: {res_version.stdout}")

    res_help = subprocess.run(
        [str(bin_path), "--help"],
        env=env,
        capture_output=True,
        text=True,
        check=True,
    )
    if res_help.returncode != 0:
        raise ValueError(f"help exited with {res_help.returncode}")


def main() -> None:
    pkg_root = Path(__file__).resolve().parent.parent
    sys.path.insert(0, str(pkg_root / "src"))
    import tandem

    version = tandem.__version__

    spec_path = pkg_root / "packaging" / "tandem.spec"
    dist_dir = pkg_root / "dist"
    work_dir = pkg_root / "build"

    run_build(spec_path, dist_dir / "tandem", work_dir)
    smoke_test(dist_dir / "tandem", version)

    os_slug, arch_slug = platform_slug()
    archive_path = create_archive(dist_dir / "tandem", dist_dir, version, os_slug, arch_slug)
    write_sha256(archive_path)

    print(str(archive_path.resolve()))


if __name__ == "__main__":
    main()
