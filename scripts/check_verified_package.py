#!/usr/bin/env python3
"""Independently extract a matched package and render its installed native UI.

Run on the package's native OS. This never uses an external runtime override.
The original archive is retained unchanged; choose a fresh evidence directory.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import tarfile
import zipfile

from portable_common import checked_member, copy_bounded
from run_validation import Validation, validate_diagnosis, validate_smoke


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def extract(archive, directory):
    directory.mkdir(parents=True, exist_ok=False)
    seen, total = set(), 0
    maximum = 2 * 1024**3
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as source:
            for member in source.infolist():
                relative = checked_member(member.filename.rstrip("/"), seen)
                mode = (member.external_attr >> 16) & 0o170000
                if mode not in (0, 0o040000, 0o100000) or member.flag_bits & 1:
                    raise ValueError("Encrypted, linked or special ZIP member")
                total += member.file_size
                if total > maximum:
                    raise ValueError("Package exceeds extraction budget")
                target = directory.joinpath(*relative.parts)
                if member.is_dir():
                    target.mkdir(parents=True, exist_ok=True)
                else:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with source.open(member) as inp, target.open("xb") as out:
                        copy_bounded(inp, out, member.file_size)
                    target.chmod(0o755 if (member.external_attr >> 16) & 0o111 else 0o644)
    else:
        with tarfile.open(archive, "r:gz") as source:
            for member in source:
                relative = checked_member(member.name.rstrip("/"), seen)
                if not (member.isfile() or member.isdir()):
                    raise ValueError("Linked or special TAR member")
                total += member.size
                if total > maximum:
                    raise ValueError("Package exceeds extraction budget")
                target = directory.joinpath(*relative.parts)
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                else:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with source.extractfile(member) as inp, target.open("xb") as out:
                        copy_bounded(inp, out, member.size)
                    target.chmod(0o755 if member.mode & 0o111 else 0o644)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--platform", choices=("macos-arm64", "windows-x86_64", "linux-x86_64"), required=True)
    parser.add_argument("--report-dir", type=Path, required=True)
    args = parser.parse_args()
    expected_host = {"macos-arm64": "Darwin", "windows-x86_64": "Windows", "linux-x86_64": "Linux"}[args.platform]
    if platform.system() != expected_host:
        raise ValueError("Package must be checked on its native operating system")
    root, archive, directory = Path(__file__).resolve().parent.parent, args.archive.resolve(), args.report_dir.resolve()
    if directory.exists():
        raise ValueError("Use a fresh package evidence directory")
    directory.mkdir(parents=True)
    report = {"status": "FAIL", "platform": args.platform, "archive": archive.name, "archive_sha256": sha256(archive)}
    try:
        extract(archive, directory / "extracted")
        manifests = list((directory / "extracted").rglob("package-manifest.json"))
        if len(manifests) != 1:
            raise ValueError("Expected exactly one package inventory")
        manifest = json.loads(manifests[0].read_text(encoding="utf-8"))
        version = manifest["version"]
        stage = manifests[0].parent
        inventory = manifest["files"]
        declared = {}
        for record in inventory:
            relative = checked_member(record["path"], set())
            path = stage.joinpath(*relative.parts)
            if record["path"] in declared or not path.is_file() or path.is_symlink():
                raise ValueError("Invalid package inventory member")
            if sha256(path) != record["sha256"] or path.stat().st_size != record["bytes"]:
                raise ValueError(f"Extracted file differs from package inventory: {relative}")
            declared[record["path"]] = record
        actual = {p.relative_to(stage).as_posix() for p in stage.rglob("*") if p.is_file() and p != manifests[0]}
        if actual != set(declared):
            raise ValueError("Package inventory is incomplete")
        if args.platform == "macos-arm64":
            candidates = list(stage.glob("*.app/Contents/MacOS/nova-flux"))
        else:
            candidates = [stage / ("nova-flux.exe" if args.platform.startswith("windows-") else "nova-flux")]
        if len(candidates) != 1 or not candidates[0].is_file():
            raise ValueError("Cannot locate the packaged application")
        binary = candidates[0]
        environment = os.environ.copy()
        for key in ("NOVA_RUNTIME", "NOVA_TEST_RUNTIME"):
            environment.pop(key, None)
        environment.update(NOVA_DATA_DIR=str(directory / "state"), NOVA_CACHE_DIR=str(directory / "cache"))
        if args.platform.startswith("linux-"):
            xdg = directory / "xdg-runtime"
            xdg.mkdir(mode=0o700)
            environment["XDG_RUNTIME_DIR"] = str(xdg)
        validation = Validation(root, directory, args.platform, version)
        diagnosis = validation.step("extracted-diagnosis", [str(binary), "--diagnose"], environment=environment, timeout=60)
        report["diagnose"] = validate_diagnosis(diagnosis, version)
        screenshot = directory / "extracted-smoke.png"
        environment.update(NOVA_SMOKE_TEST="1", NOVA_SMOKE_TEST_MS="3000", NOVA_SMOKE_SCREENSHOT=str(screenshot))
        output = validation.step("extracted-smoke", [str(binary), "--smoke-test"], environment=environment, timeout=60)
        report["native_gui"] = validate_smoke(output, screenshot, version, args.platform)
        report.update(status="PASS", version=version, verified_files=len(inventory), binary_sha256=sha256(binary))
        validation.report.update(status="PASS", package_check=report)
        validation.save()
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        (directory / "package-check.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({"status": "PASS", "report": str(directory / "package-check.json")}))


if __name__ == "__main__":
    main()
