#!/usr/bin/env python3
"""Export allowlisted public acceptance evidence; never copy logs or host paths.

This summarizes checks performed by the native validation and verified-package
drivers. It is not a substitute for those checks or a release-publishing tool.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import tomllib

from run_validation import png_info

ROOT = Path(__file__).resolve().parent.parent
PLATFORMS = ("macos-arm64", "windows-x86_64", "linux-x86_64")
STATUSES = {"PASS", "FAIL", "RUNNING", "NOT_RUN", "INCOMPLETE"}


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def read_json(path: Path) -> dict:
    if not path.exists():
        return {"status": "NOT_RUN"}
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 8 * 1024 * 1024:
        raise ValueError("Evidence input is not a bounded regular JSON file")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError("Evidence input must be a JSON object")
    return value


def status(value: dict) -> str:
    result = value.get("status", "NOT_RUN")
    return result if result in STATUSES else "FAIL"


def integer_fields(value: dict, names: tuple[str, ...]) -> dict:
    return {name: value[name] for name in names if type(value.get(name)) is int and value[name] >= 0}


def gui_summary(value: dict) -> dict:
    return {"status": status(value), **integer_fields(value, ("frames",)), "image": {
        **integer_fields(value.get("image", {}), ("width", "height", "bytes")),
        **({"sha256": value["image"]["sha256"]} if re.fullmatch(r"[0-9a-f]{64}", str(value.get("image", {}).get("sha256", ""))) else {}),
    }}


def source_summary(report: dict) -> dict:
    result = {"status": status(report), "tests": integer_fields(report.get("tests", {}), ("executables", "passed", "failed", "ignored")),
              "native_gui": gui_summary(report.get("native-smoke", {})), "steps": []}
    commit = report.get("commit", "")
    if isinstance(commit, str) and re.fullmatch(r"[0-9a-f]{40,64}", commit):
        result["commit"] = commit
    host = report.get("host", {})
    result["host"] = {key: host[key] for key in ("os", "architecture", "python")
                      if isinstance(host.get(key), str) and re.fullmatch(r"[A-Za-z0-9_.+-]{1,40}", host[key])}
    for step in report.get("steps", []):
        name = step.get("name", "")
        if not isinstance(name, str) or not re.fullmatch(r"[a-z0-9-]{1,80}", name):
            raise ValueError("Unrecognized validation step name")
        result["steps"].append({"name": name, "status": status(step),
                                **integer_fields(step, ("exit_code",)),
                                **({"timed_out": True} if step.get("timed_out") is True else {})})
    return result


def copy_screenshot(source: Path, output: Path, evidence: dict) -> None:
    if evidence.get("status") != "PASS":
        return
    if source.is_symlink():
        raise ValueError("Screenshot input must not be a symlink")
    actual = png_info(source)
    if actual != evidence.get("image") or type(evidence.get("frames")) is not int or evidence["frames"] < 3:
        raise ValueError("Screenshot bytes do not match completed native GUI evidence")
    shutil.copyfile(source, output)


def export_evidence(platform: str, validation_dir: Path, extracted_dir: Path, packages_dir: Path, source_bundle: Path, output: Path) -> dict:
    if platform not in PLATFORMS:
        raise ValueError("Unsupported public evidence platform")
    if output.exists() or output.is_symlink():
        raise ValueError("Use a fresh public evidence directory")
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?", version):
        raise ValueError("Application version is not a portable archive component")
    native = read_json(validation_dir / "report.json")
    extracted = read_json(extracted_dir / "package-check.json")
    summary = {"schema_version": 1, "scope": "Fresh native matched-source validation; not a published release", "platform": platform, "version": version,
               "native": source_summary(native),
               "extracted": {"status": status(extracted), **integer_fields(extracted, ("verified_files",)), "native_gui": gui_summary(extracted.get("native_gui", {}))}}
    output.mkdir(parents=True)
    copy_screenshot(validation_dir / "native-smoke.png", output / "native-smoke.png", summary["native"]["native_gui"])
    copy_screenshot(extracted_dir / "extracted-smoke.png", output / "extracted-smoke.png", summary["extracted"]["native_gui"])
    (output / "validation.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    receipt = {"schema_version": 1, "scope": summary["scope"], "platform": platform, "version": version, "status": "INCOMPLETE"}
    complete = native.get("status") == extracted.get("status") == "PASS"
    if complete:
        if (native.get("platform"), native.get("app_version"), extracted.get("platform"), extracted.get("version")) != (platform, version, platform, version):
            raise ValueError("Native or extracted acceptance describes another version/platform")
        tests = summary["native"]["tests"]
        if not tests.get("passed") or not tests.get("executables") or tests.get("failed") != 0 or tests.get("ignored") != 0:
            raise ValueError("Native tests must all pass with none ignored")
        if any(summary[part]["native_gui"]["status"] != "PASS" for part in ("native", "extracted")):
            raise ValueError("Native and extracted GUI acceptance must pass")
        name = f"Nova-Flux-{version}-{platform}"
        suffix = ".tar.gz" if platform == "linux-x86_64" else ".zip"
        archive = packages_dir / (name + suffix)
        manifest_path = packages_dir / name / "package-manifest.json"
        manifest = read_json(manifest_path)
        for path in (archive, source_bundle, manifest_path):
            if path.is_symlink() or not path.is_file():
                raise ValueError("Complete acceptance requires regular final archive/source/manifest inputs")
        archive_sha, source_sha = digest(archive), digest(source_bundle)
        binding = manifest.get("source_bundle", {})
        source_name = f"Nova-Flux-{version}-FFmpeg-sources.zip"
        if source_bundle.name != source_name or binding.get("filename") != source_name or binding.get("sha256") != source_sha:
            raise ValueError("Package manifest does not bind the exact corresponding-source ZIP")
        if (manifest.get("version"), manifest.get("platform"), manifest.get("distribution_status")) != (version, platform, "verified-source-runtime-package"):
            raise ValueError("Package manifest describes another version/platform or package channel")
        if extracted.get("archive") != archive.name or extracted.get("archive_sha256") != archive_sha or extracted.get("binary_sha256") != manifest.get("packaged_application_sha256"):
            raise ValueError("Extracted GUI acceptance does not bind the final archive/application")
        source_record = {"filename": source_name, "sha256": source_sha, "bytes": source_bundle.stat().st_size}
        for key in ("builder_sha256", "sources_lock_sha256"):
            if not re.fullmatch(r"[0-9a-f]{64}", str(binding.get(key, ""))):
                raise ValueError("Package source recipe hash is missing")
            source_record[key] = binding[key]
        source_set = binding.get("source_set_id", "")
        if not isinstance(source_set, str) or not re.fullmatch(r"[A-Za-z0-9_.+-]{1,100}", source_set):
            raise ValueError("Package source-set identifier is missing")
        source_record["source_set_id"] = source_set
        receipt.update(status="PASS", archive={"filename": archive.name, "sha256": archive_sha, "bytes": archive.stat().st_size},
                       source_bundle=source_record, package_manifest_sha256=digest(manifest_path),
                       native_tests=tests, native_gui=summary["native"]["native_gui"], extracted_gui=summary["extracted"]["native_gui"])
        if "commit" in summary["native"]:
            receipt["commit"] = summary["native"]["commit"]
        source_bundle.with_name(source_bundle.name + ".sha256").write_text(source_sha + "  " + source_name + "\n", encoding="ascii")
    elif "FAIL" in (status(native), status(extracted)):
        receipt["status"] = "FAIL"
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return receipt


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=PLATFORMS, required=True)
    for name in ("validation-dir", "extracted-dir", "packages-dir", "source-bundle", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    receipt = export_evidence(args.platform, args.validation_dir, args.extracted_dir, args.packages_dir, args.source_bundle, args.output)
    print(json.dumps({"platform": args.platform, "status": receipt["status"]}))
    return 0 if receipt["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
