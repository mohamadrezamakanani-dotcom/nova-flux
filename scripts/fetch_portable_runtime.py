#!/usr/bin/env python3
"""Acquire a hash-pinned FFmpeg runtime for private native CI tests only.

This is a build-time operation, never called by the installed application.
No public redistribution is approved by this script or its provenance lock.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import shutil
import tempfile
import urllib.request

from portable_common import (
    PLATFORMS, PRIVATE_NOTICE, binary_architecture, files_manifest,
    inspect_runtime, json_write, require_host, run, safe_extract, sha256,
)


def download(asset: dict, cache: Path) -> Path:
    cache.mkdir(parents=True, exist_ok=True)
    archive = cache / asset["archive"]
    expected_size = asset["bytes"]
    if archive.exists():
        if archive.stat().st_size == expected_size and sha256(archive) == asset["sha256"]:
            return archive
        raise ValueError("Cached runtime archive differs from the pinned hash; remove that archive and retry")
    if not asset["url"].startswith("https://github.com/BtbN/FFmpeg-Builds/releases/download/"):
        raise ValueError("The runtime lock must point to the upstream HTTPS release asset")
    request = urllib.request.Request(asset["url"], headers={"User-Agent": "Nova-Flux-private-CI-runtime/1"})
    with tempfile.NamedTemporaryFile(dir=cache, prefix=".download-", delete=False) as temp:
        temporary = Path(temp.name)
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                if not response.geturl().startswith("https://"):
                    raise ValueError("Runtime download redirected to an insecure URL")
                received = 0
                while True:
                    chunk = response.read(1024 * 1024)
                    if not chunk:
                        break
                    received += len(chunk)
                    if received > expected_size:
                        raise ValueError("Runtime archive exceeds its pinned byte length")
                    temp.write(chunk)
            temp.flush()
            if received != expected_size or sha256(temporary) != asset["sha256"]:
                raise ValueError("Runtime download failed its pinned byte length or SHA-256 check")
            temp.close()
            temporary.replace(archive)
        finally:
            temp.close()
            temporary.unlink(missing_ok=True)
    return archive


def acquire(target: str, destination: Path, cache: Path, lock_path: Path) -> Path:
    require_host(target)
    lock = json.loads(lock_path.read_text(encoding="utf-8"))
    if lock.get("schema") != 1 or lock.get("distribution_status") != "private-ci-only":
        raise ValueError("Only the private-CI runtime lock schema is supported")
    asset = lock["platforms"][target]
    destination = destination.absolute()
    if destination.exists():
        raise ValueError("Refusing to overwrite an existing runtime directory")
    destination.parent.mkdir(parents=True, exist_ok=True)
    archive = download(asset, cache.absolute())
    with tempfile.TemporaryDirectory(prefix=".nova-runtime-", dir=destination.parent) as scratch:
        scratch = Path(scratch)
        extracted = scratch / "upstream"
        safe_extract(archive, extracted, asset["format"])
        roots = list(extracted.iterdir())
        if len(roots) != 1 or not roots[0].is_dir():
            raise ValueError("Expected exactly one upstream runtime root directory")
        runtime = scratch / "runtime"
        shutil.copytree(roots[0], runtime)
        # ffplay is outside Nova Flux's feature set and is not shipped.
        for name in ("ffplay", "ffplay.exe"):
            (runtime / "bin" / name).unlink(missing_ok=True)
        license_file = runtime / "LICENSE.txt"
        if not license_file.is_file() or not license_file.stat().st_size:
            raise ValueError("The upstream binary archive lacks its original LICENSE.txt")
        for tool in (runtime / "bin").iterdir():
            if tool.is_file() and (tool.suffix.lower() in (".exe", ".dll") or not tool.suffix):
                if binary_architecture(tool) != asset["binary_format"]:
                    raise ValueError("Runtime contains a binary for an unexpected architecture")
                if target.startswith("linux-"):
                    tool.chmod(0o755)
        inspection = inspect_runtime(runtime, target, lock["ffmpeg_version_prefix"])
        provenance = runtime / "provenance"
        provenance.mkdir()
        shutil.copyfile(lock_path, provenance / "upstream-lock.json")
        (runtime / "PRIVATE_CI_ONLY.txt").write_text(PRIVATE_NOTICE, encoding="utf-8")
        (provenance / "ffmpeg-license-output.txt").write_text(
            run([str(runtime / "bin" / ("ffmpeg.exe" if target.startswith("windows-") else "ffmpeg")), "-hide_banner", "-L"]), encoding="utf-8")
        (provenance / "SOURCE_STATUS.txt").write_text(
            lock["source_status"] + "\n\n"
            + "FFmpeg source revision: https://github.com/FFmpeg/FFmpeg/tree/" + lock["ffmpeg_commit"] + "\n"
            + "Builder recipe: https://github.com/BtbN/FFmpeg-Builds/tree/" + lock["builder_commit"] + "\n"
            + "The builder recipe describes optional dependencies. This runtime does not contain their complete corresponding source.\n",
            encoding="utf-8")
        manifest = {
            "schema": 1, "platform": target, "distribution_status": "private-ci-only",
            "provider": lock["provider"], "release_tag": lock["release_tag"],
            "builder_commit": lock["builder_commit"], "ffmpeg_commit": lock["ffmpeg_commit"],
            "ffmpeg_version_prefix": lock["ffmpeg_version_prefix"],
            "archive": asset, "source_status": lock["source_status"],
            "inspection": inspection,
            "files": files_manifest(runtime),
        }
        json_write(runtime / "manifest.json", manifest)
        runtime.rename(destination)
    return destination


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=PLATFORMS, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--cache", type=Path, default=Path(".runtime-downloads"))
    parser.add_argument("--lock", type=Path, default=Path(__file__).with_name("portable_runtime.lock.json"))
    args = parser.parse_args()
    runtime = acquire(args.platform, args.destination, args.cache, args.lock)
    print(json.dumps({"runtime": str(runtime), "platform": args.platform, "status": "PASS", "distribution_status": "private-ci-only"}))


if __name__ == "__main__":
    main()
