#!/usr/bin/env python3
"""Bind three native acceptance receipts to this commit and the common source ZIP.

Native package bytes were checked in their producing jobs. This aggregation
downloads only receipts and the common source ZIP, never the native packages.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parent.parent
PLATFORMS = {"macos-arm64", "windows-x86_64", "linux-x86_64"}


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def checked_hash(value) -> str:
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value):
        raise ValueError("Receipt hash is missing or malformed")
    return value


def aggregate(receipts: list[Path], source_bundle: Path, output: Path, *, expected_commit: str, root: Path = ROOT) -> dict:
    if len(receipts) != 3:
        raise ValueError("Exactly three native receipts are required")
    if not re.fullmatch(r"[0-9a-f]{40,64}", expected_commit):
        raise ValueError("Expected source commit is missing or malformed")
    if output.exists() or output.is_symlink():
        raise ValueError("Use a fresh aggregate evidence directory")
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?", version):
        raise ValueError("Application version is not a portable archive component")
    recipe = root / "scripts/runtime-source"
    lock = json.loads((recipe / "sources.lock.json").read_text(encoding="utf-8"))
    source_set = lock["source_set_id"]
    if not isinstance(source_set, str) or not re.fullmatch(r"[A-Za-z0-9_.+-]{1,100}", source_set):
        raise ValueError("Source lock identifier is malformed")
    source_name = f"Nova-Flux-{version}-FFmpeg-sources.zip"
    if source_bundle.name != source_name or source_bundle.is_symlink() or not source_bundle.is_file():
        raise ValueError("Expected a regular common source ZIP for this version")
    source_record = {"filename": source_name, "sha256": digest(source_bundle), "bytes": source_bundle.stat().st_size,
                     "builder_sha256": digest(recipe / "build_runtime.py"), "sources_lock_sha256": digest(recipe / "sources.lock.json"), "source_set_id": source_set}
    platforms, seen = [], set()
    for path in receipts:
        if path.is_symlink() or not path.is_file() or path.stat().st_size > 1024 * 1024:
            raise ValueError("Expected a bounded regular native receipt")
        receipt = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(receipt, dict) or receipt.get("schema_version") != 1 or receipt.get("status") != "PASS":
            raise ValueError("Every native receipt must have the supported schema and PASS status")
        platform = receipt.get("platform")
        if platform not in PLATFORMS or platform in seen:
            raise ValueError("Receipts must identify three unique supported native platforms")
        seen.add(platform)
        if receipt.get("version") != version or receipt.get("commit") != expected_commit:
            raise ValueError("Receipt describes another application version or source commit")
        binding = receipt.get("source_bundle", {})
        if any(binding.get(key) != value for key, value in source_record.items()):
            raise ValueError("Native source binding differs from common ZIP or checked-out frozen recipe")
        tests = receipt.get("native_tests", {})
        if any(type(tests.get(key)) is not int or tests[key] < 0 for key in ("executables", "passed", "failed", "ignored")):
            raise ValueError("Receipt is missing complete native test counts")
        if tests["executables"] == 0 or tests["passed"] == 0 or tests["failed"] != 0 or tests["ignored"] != 0:
            raise ValueError("Native tests must all pass with none ignored")
        archive = receipt.get("archive", {})
        extension = ".tar.gz" if platform == "linux-x86_64" else ".zip"
        archive_name = f"Nova-Flux-{version}-{platform}{extension}"
        if archive.get("filename") != archive_name or type(archive.get("bytes")) is not int or archive["bytes"] <= 0:
            raise ValueError("Receipt archive name or size does not describe this native package")
        platforms.append({"platform": platform, "archive": {"filename": archive_name, "sha256": checked_hash(archive.get("sha256")), "bytes": archive["bytes"]},
                          "package_manifest_sha256": checked_hash(receipt.get("package_manifest_sha256")),
                          "native_tests": {key: tests[key] for key in ("executables", "passed", "failed", "ignored")}})
    if seen != PLATFORMS:
        raise ValueError("A supported native platform is missing")
    platforms.sort(key=lambda item: item["platform"])
    report = {"schema_version": 1, "status": "PASS", "version": version, "commit": expected_commit,
              "scope": "Three successful native matched-source acceptance receipts and the actual common source ZIP agree",
              "source_bundle": source_record, "platforms": platforms,
              "package_byte_verification": "Native package bytes were validated upstream; package archives were not downloaded by this aggregation job. Verify downloaded packages against SHA256SUMS."}
    checksums = {source_name: source_record["sha256"]}
    checksums.update({item["archive"]["filename"]: item["archive"]["sha256"] for item in platforms})
    output.mkdir(parents=True)
    (output / "report.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (output / "SHA256SUMS").write_text("".join(f"{checksums[name]}  {name}\n" for name in sorted(checksums)), encoding="ascii")
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", type=Path, action="append", required=True)
    parser.add_argument("--source-bundle", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--expected-commit", required=True)
    args = parser.parse_args()
    report = aggregate(args.receipt, args.source_bundle, args.output, expected_commit=args.expected_commit)
    print(json.dumps({"status": report["status"], "platforms": len(report["platforms"]), "source_sha256": report["source_bundle"]["sha256"]}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
