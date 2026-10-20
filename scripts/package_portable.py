#!/usr/bin/env python3
"""Create native Windows/Linux private-CI portable archives, with checksums.

Public release packaging is deliberately unsupported until matching FFmpeg
and dependency source provenance has been assembled and verified.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import tarfile
import tempfile
import zipfile

from portable_common import (
    PLATFORMS, PRIVATE_NOTICE, binary_architecture, files_manifest,
    inspect_runtime, json_write, require_host, run, sha256, verify_files,
)


def package(target: str, runtime: Path, binary: Path, destination: Path, channel: str) -> Path:
    if channel != "private-ci":
        raise ValueError("Public binary packaging is blocked: complete corresponding runtime source is not verified")
    require_host(target)
    runtime = runtime.resolve(strict=True)
    binary = binary.resolve(strict=True)
    manifest = json.loads((runtime / "manifest.json").read_text(encoding="utf-8"))
    if manifest.get("schema") != 1 or manifest.get("distribution_status") != "private-ci-only" or manifest.get("platform") != target:
        raise ValueError("Runtime provenance does not match this private-CI target")
    verify_files(runtime, manifest["files"], exclude=("manifest.json",))
    expected = "pe-x86_64" if target.startswith("windows-") else "elf-x86_64"
    if binary_architecture(binary) != expected:
        raise ValueError("Application architecture does not match the native package target")
    root = Path(__file__).resolve().parent.parent
    version_match = re.search(r'^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?)"', (root / "Cargo.toml").read_text(encoding="utf-8"), re.M)
    if not version_match:
        raise ValueError("Cannot read the application package version")
    version = version_match.group(1)
    package_name = f"Nova-Flux-{version}-{target}-PRIVATE-CI"
    destination = destination.absolute()
    destination.mkdir(parents=True, exist_ok=True)
    folder = destination / package_name
    archive = destination / (package_name + (".zip" if target.startswith("windows-") else ".tar.gz"))
    checksum = archive.with_name(archive.name + ".sha256")
    if any(path.exists() for path in (folder, archive, checksum)):
        raise ValueError("Refusing to overwrite an existing portable package or checksum")
    with tempfile.TemporaryDirectory(prefix=".nova-package-", dir=destination) as scratch:
        stage = Path(scratch) / package_name
        stage.mkdir()
        executable = stage / ("nova-flux.exe" if target.startswith("windows-") else "nova-flux")
        shutil.copy2(binary, executable)
        if target.startswith("linux-"):
            executable.chmod(0o755)
        shutil.copytree(runtime, stage / "runtime")
        shutil.copyfile(root / "assets" / "icon.png", stage / "icon.png")
        for name in ("LICENSE", "LICENSE.txt", "THIRD_PARTY_NOTICES.md"):
            if (root / name).is_file():
                shutil.copyfile(root / name, stage / name)
        (stage / "PRIVATE_CI_ONLY.txt").write_text(PRIVATE_NOTICE, encoding="utf-8")
        (stage / "README.txt").write_text(
            f"Nova Flux {version} — {target} — PRIVATE CI VALIDATION\n\n"
            + "Extract this entire folder before opening nova-flux" + (".exe" if target.startswith("windows-") else "") + ".\n"
            + "Keep runtime/ beside the application. No network access is needed at launch.\n"
            + "The FFmpeg components retain their original licenses; see runtime/LICENSE.txt.\n"
            + "This test package is unsigned. It is not a supported public release.\n"
            + "Native CI evidence does not establish compatibility with every OS version or GPU.\n"
            + "See PRIVATE_CI_ONLY.txt and runtime/provenance/SOURCE_STATUS.txt.\n",
            encoding="utf-8")
        inspection = inspect_runtime(stage / "runtime", target, manifest["ffmpeg_version_prefix"])
        environment = os.environ.copy()
        environment.pop("NOVA_RUNTIME", None)
        # The installed app, not a development override, must find runtime/.
        diagnose = json.loads(run([str(executable), "--diagnose"], env=environment))
        if diagnose.get("ready") is not True:
            raise ValueError("Packaged app runtime diagnosis did not report ready=true")
        if diagnose.get("app_version") != version:
            raise ValueError("Packaged application version differs from Cargo.toml; rebuild the release binary")
        json_write(stage / "package-manifest.json", {
            "schema": 1, "application": "Nova Flux", "version": version,
            "platform": target, "distribution_status": "private-ci-only",
            "binary_sha256": sha256(executable), "runtime_inspection": inspection,
            "app_diagnose": diagnose, "files": files_manifest(stage),
        })
        temporary_archive = Path(scratch) / archive.name
        if target.startswith("windows-"):
            with zipfile.ZipFile(temporary_archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as output:
                for path in sorted(stage.rglob("*")):
                    if path.is_file():
                        output.write(path, path.relative_to(stage.parent).as_posix())
            with zipfile.ZipFile(temporary_archive) as output:
                if output.testzip() is not None:
                    raise ValueError("Packaged ZIP failed CRC verification")
        else:
            with tarfile.open(temporary_archive, "w:gz", compresslevel=6) as output:
                output.add(stage, arcname=package_name, recursive=True)
            with tarfile.open(temporary_archive, "r:gz") as output:
                names = output.getnames()
                if f"{package_name}/nova-flux" not in names or f"{package_name}/runtime/bin/ffmpeg" not in names:
                    raise ValueError("Packaged tar archive lacks expected executables")
        digest = sha256(temporary_archive)
        stage.rename(folder)
        temporary_archive.rename(archive)
        checksum.write_text(digest + "  " + archive.name + "\n", encoding="ascii")
    return archive


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=PLATFORMS, required=True)
    parser.add_argument("--runtime", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--channel", choices=("private-ci",), required=True)
    args = parser.parse_args()
    archive = package(args.platform, args.runtime, args.binary, args.destination, args.channel)
    print(json.dumps({"archive": str(archive), "sha256": sha256(archive), "distribution_status": "private-ci-only", "status": "PASS"}))


if __name__ == "__main__":
    main()
