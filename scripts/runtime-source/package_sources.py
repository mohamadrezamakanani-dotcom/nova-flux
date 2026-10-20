#!/usr/bin/env python3
"""Create or verify Nova's deterministic, complete media-runtime source ZIP."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import stat
import sys
import tarfile
import tempfile
import zipfile


HERE = Path(__file__).resolve().parent
REPOSITORY = HERE.parents[1]
PREFIX = "scripts/runtime-source/"
SOURCE_NAMES = {"ffmpeg", "x264", "lame", "libogg", "libvorbis", "opus", "libvpx", "dav1d", "zlib"}
PLATFORMS = {"macos-arm64", "linux-x86_64", "windows-x86_64"}
RECIPE_FILES = ("sources.lock.json", "acquire_sources.py", "build_runtime.py", "BUILD.md")
SUPPORT_FILES = ("package_sources.py", "SOURCE_BUNDLE.md", "test_acquire_sources.py", "test_package_sources.py")
MANDATORY_PATHS = {"LICENSE", "COPYING.GPLv3", "INTRODUCTION.md", *(PREFIX + name for name in RECIPE_FILES + SUPPORT_FILES)}
MANIFEST_NAME = "BUNDLE-MANIFEST.json"
FIXED_TIMESTAMP = (1980, 1, 1, 0, 0, 0)

SPEC = importlib.util.spec_from_file_location("nova_acquire_sources", HERE / "acquire_sources.py")
acquire = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(acquire)
SourceError = acquire.SourceError


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def json_bytes(value: object) -> bytes:
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def file_record(path: str, data: bytes) -> dict:
    return {"path": path, "bytes": len(data), "sha256": digest(data)}


def inventory_digest(records: list) -> str:
    value = sorted(records, key=lambda entry: entry["path"])
    return digest(json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode("utf-8"))


def read_regular(path: Path) -> bytes:
    if path.is_symlink() or not path.is_file():
        raise SourceError(f"Missing regular bundle input: {path}")
    return path.read_bytes()


def check_source_set(lock: dict) -> None:
    if len(lock["sources"]) != 9 or {source["name"] for source in lock["sources"]} != SOURCE_NAMES:
        raise SourceError("A source bundle must retain exactly the nine locked runtime dependencies")
    if lock.get("patches") or any(source.get("patches") for source in lock["sources"]):
        raise SourceError("This bundle format requires the current unmodified-source recipe; patches need explicit packaging support")
    acquire.safe_path(lock["source_set_id"], leaf=True)


def archive_licenses(data: bytes, source: dict) -> dict:
    if len(data) != source["archive_bytes"] or digest(data) != source["sha256"]:
        raise SourceError(f"Archive does not match locked original bytes: {source['archive']}")
    expected = {item["path"]: item for item in source["license"]["file_hashes"]}
    if set(expected) != set(source["license"]["files"]):
        raise SourceError(f"Inconsistent license inventory: {source['name']}")
    found = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:*") as archive:
        for member, relative in acquire.inspected_members(archive, source):
            name = relative.as_posix()
            if member.isfile() and name in expected:
                content = archive.extractfile(member).read()
                item = expected[name]
                if len(content) != item["bytes"] or digest(content) != item["sha256"]:
                    raise SourceError(f"License differs from locked upstream text: {source['name']}/{name}")
                found[name] = content
    if set(found) != set(expected):
        raise SourceError(f"Missing locked upstream license text: {source['name']}")
    return found


def introduction(lock: dict) -> bytes:
    rows = "\n".join(f"| {s['name']} | {s['version']} | {s['license']['expression']} |" for s in lock["sources"])
    text = f"""# Nova FFmpeg runtime: complete retained source

Source set: `{lock['source_set_id']}`.

This bundle contains the nine complete original upstream source archives,
their license texts, the exact source lock, and the scripts and instructions
for configuring and building Nova's FFmpeg and ffprobe runtime. Original archive
bytes are retained without repacking. No upstream source patches are applied.

| Component | Pinned version | Runtime dependency license |
| --- | --- | --- |
{rows}

FFmpeg is configured with `--enable-gpl --enable-version3` and without nonfree
components. The combined runtime uses GPL-3.0-or-later; its full GPL v3 text is
`COPYING.GPLv3`. Dependency copyright/license texts are in `licenses/` and in
the complete original archives. Root `LICENSE` is the Nova MIT license for its
own scripts/application code; it does not replace dependency licenses.

From the unpacked bundle root, verify every retained archive and license:

```sh
python3 scripts/runtime-source/acquire_sources.py --cache-dir . --offline --verify-only
```

To build on a native supported host, install the tools listed in
`scripts/runtime-source/BUILD.md`, then use a new work and output directory:

```sh
python3 scripts/runtime-source/build_runtime.py --platform macos-arm64 --sources . --work-dir ../nova-build --destination ../nova-runtime --jobs 8
```

Select `linux-x86_64` or `windows-x86_64` on the corresponding native host.
Windows requires MSYS2 UCRT64 and its native Python/compiler. Python's standard
library runs these drivers; compilers, SDK/system libraries, Bash, Make,
CMake, Meson, Ninja, pkg-config and x86 NASM are build prerequisites, not
redistributed compiler binaries. No prebuilt media runtime is required.

The lock records exact HTTPS source URLs, upstream references, original archive
SHA-256 values and license-file hashes. The retained archives support an
offline build after installing the native tools. `BUILD.md` describes flags,
toolchain boundaries and runtime checks. `SOURCE_BUNDLE.md` beside it describes
bundle verification. Optional `build-manifests/` files record matching native
builds; their absence does not remove any source or recipe required to build.

`BUNDLE-MANIFEST.json` inventories every payload file by size and SHA-256 and
identifies the exact builder and lock. Check a separately published ZIP SHA-256
when authenticity matters. Internal hash consistency is not a signature,
legal certification, binary reproducibility guarantee, application acceptance,
or proof that a platform build has run.
"""
    return text.encode("utf-8")


def validate_build_manifest(data: bytes, lock: dict, lock_sha: str, builder_sha: str) -> str:
    value = json.loads(data)
    platform = value.get("platform")
    if platform not in PLATFORMS or value.get("source_set_id") != lock["source_set_id"]:
        raise SourceError("Build manifest has an unsupported platform or a different source set")
    if value.get("sources_lock_sha256") != lock_sha or value.get("builder_sha256") != builder_sha:
        raise SourceError("Build manifest does not match exact bundled lock/builder bytes")
    if value.get("patches"):
        raise SourceError("Build manifest contains unbundled source patches")
    return platform


def collect_payload(cache: Path, recipe_dir: Path, repository: Path, build_manifests: list[Path]) -> tuple:
    lock_path = recipe_dir / "sources.lock.json"
    lock = acquire.load_lock(lock_path)
    check_source_set(lock)
    files = {PREFIX + name: read_regular(recipe_dir / name) for name in RECIPE_FILES + SUPPORT_FILES}
    # Parse the same retained lock bytes that enter the bundle, avoiding a race
    # between an earlier validation read and recipe collection.
    if json.loads(files[PREFIX + "sources.lock.json"]) != lock:
        raise SourceError("Source lock changed while collecting bundle inputs")
    files["LICENSE"] = read_regular(repository / "LICENSE")
    files["INTRODUCTION.md"] = introduction(lock)
    for source in lock["sources"]:
        archive = read_regular(cache / "archives" / source["archive"])
        licenses = archive_licenses(archive, source)
        files["archives/" + source["archive"]] = archive
        files.update({f"licenses/{source['name']}/{path}": content for path, content in licenses.items()})
    files["COPYING.GPLv3"] = files["licenses/ffmpeg/COPYING.GPLv3"]
    lock_sha, builder_sha = digest(files[PREFIX + "sources.lock.json"]), digest(files[PREFIX + "build_runtime.py"])
    for path in build_manifests:
        data = read_regular(path)
        platform = validate_build_manifest(data, lock, lock_sha, builder_sha)
        key = f"build-manifests/{platform}.json"
        if key in files:
            raise SourceError(f"Duplicate platform build manifest: {platform}")
        files[key] = data
    recipes = [file_record(PREFIX + name, files[PREFIX + name]) for name in RECIPE_FILES]
    sources = [file_record("archives/" + s["archive"], files["archives/" + s["archive"]]) for s in lock["sources"]]
    manifest = {
        "schema_version": 1,
        "source_set_id": lock["source_set_id"],
        "top_directory": lock["source_set_id"] + "-sources",
        "inventory_digest_algorithm": "sha256-compact-sorted-keys-utf8-json-records-sorted-by-path-v1",
        "source_set_sha256": inventory_digest(sources),
        "recipe_sha256": inventory_digest(recipes),
        "sources_lock_sha256": lock_sha,
        "builder_sha256": builder_sha,
        "recipe_files": recipes,
        "source_archives": sources,
        "patches": [],
        "included_platform_builds": sorted(path.split("/")[1][:-5] for path in files if path.startswith("build-manifests/")),
        "files": [file_record(path, files[path]) for path in sorted(files)],
        "integrity_note": "This manifest excludes itself. Verify a separately published ZIP SHA-256 for an external integrity anchor; no signature or certification is implied.",
    }
    files[MANIFEST_NAME] = json_bytes(manifest)
    return files, manifest


def verify_payload(files: dict, manifest: dict) -> None:
    if manifest.get("schema_version") != 1 or not isinstance(manifest.get("files"), list):
        raise SourceError("Unsupported bundle manifest schema")
    expected = {}
    for entry in manifest["files"]:
        acquire.safe_path(entry["path"])
        if entry["path"] in expected or entry["path"] == MANIFEST_NAME:
            raise SourceError("Duplicate or self-referential bundle inventory entry")
        expected[entry["path"]] = entry
    if set(files) != set(expected) | {MANIFEST_NAME} or not MANDATORY_PATHS <= set(files):
        raise SourceError("Bundle has missing, extra, or unlisted payload files")
    for path, entry in expected.items():
        if entry != file_record(path, files[path]):
            raise SourceError(f"Bundle file integrity mismatch: {path}")
    lock = json.loads(files[PREFIX + "sources.lock.json"])
    check_source_set(lock)
    if manifest.get("source_set_id") != lock["source_set_id"] or manifest.get("top_directory") != lock["source_set_id"] + "-sources":
        raise SourceError("Bundle top directory/source set differs from its lock")
    archives = {"archives/" + source["archive"] for source in lock["sources"]}
    if {path for path in files if path.startswith("archives/")} != archives:
        raise SourceError("Bundle must contain exactly the lock-selected original archives")
    required_licenses = set()
    for source in lock["sources"]:
        for relative, content in archive_licenses(files["archives/" + source["archive"]], source).items():
            key = f"licenses/{source['name']}/{relative}"
            required_licenses.add(key)
            if files.get(key) != content:
                raise SourceError(f"Bundled license differs from original archive: {key}")
    if {path for path in files if path.startswith("licenses/")} != required_licenses:
        raise SourceError("Bundle license inventory is incomplete or contains unlisted dependency texts")
    if files["COPYING.GPLv3"] != files["licenses/ffmpeg/COPYING.GPLv3"]:
        raise SourceError("Root GPL copy differs from retained upstream GPL text")
    recipes = [file_record(PREFIX + name, files[PREFIX + name]) for name in RECIPE_FILES]
    source_records = [file_record("archives/" + s["archive"], files["archives/" + s["archive"]]) for s in lock["sources"]]
    for key, value in (("recipe_files", recipes), ("source_archives", source_records), ("recipe_sha256", inventory_digest(recipes)), ("source_set_sha256", inventory_digest(source_records)), ("sources_lock_sha256", digest(files[PREFIX + "sources.lock.json"])), ("builder_sha256", digest(files[PREFIX + "build_runtime.py"]))):
        if manifest.get(key) != value:
            raise SourceError(f"Bundle provenance does not match retained bytes: {key}")
    platforms = []
    for path, data in files.items():
        if path.startswith("build-manifests/"):
            platform = validate_build_manifest(data, lock, manifest["sources_lock_sha256"], manifest["builder_sha256"])
            if path != f"build-manifests/{platform}.json":
                raise SourceError("Build manifest platform differs from its bundle path")
            platforms.append(platform)
    if sorted(platforms) != manifest.get("included_platform_builds") or manifest.get("patches") != []:
        raise SourceError("Bundle platform/patch inventory mismatch")


def verify_zip(path: Path, expected_sha256: str | None = None) -> dict:
    if path.is_symlink() or not path.is_file():
        raise SourceError(f"Missing regular source bundle ZIP: {path}")
    if expected_sha256 and acquire.sha256_file(path) != expected_sha256.lower():
        raise SourceError("ZIP SHA-256 differs from expected published value")
    files, top, seen = {}, None, set()
    total = 0
    with zipfile.ZipFile(path) as archive:
        for entry in archive.infolist():
            member = acquire.safe_path(entry.filename)
            if len(member.parts) < 2 or entry.is_dir() or not stat.S_ISREG(entry.external_attr >> 16):
                raise SourceError(f"Unexpected ZIP entry type/path: {entry.filename}")
            if top is None:
                top = member.parts[0]
            if member.parts[0] != top or entry.filename.casefold() in seen:
                raise SourceError("ZIP has multiple roots or duplicate/colliding paths")
            seen.add(entry.filename.casefold())
            total += entry.file_size
            if total > 200_000_000 or len(seen) > 1000:
                raise SourceError("Source bundle exceeds verification limits")
            relative = "/".join(member.parts[1:])
            files[relative] = archive.read(entry)
    if MANIFEST_NAME not in files:
        raise SourceError("Source bundle manifest missing")
    manifest = json.loads(files[MANIFEST_NAME])
    if manifest.get("top_directory") != top:
        raise SourceError("ZIP root differs from declared source bundle root")
    verify_payload(files, manifest)
    return manifest


def write_zip(path: Path, top: str, files: dict) -> None:
    with zipfile.ZipFile(path, "w", compression=zipfile.ZIP_STORED, allowZip64=True) as archive:
        for relative in sorted(files):
            entry = zipfile.ZipInfo(top + "/" + relative, date_time=FIXED_TIMESTAMP)
            entry.create_system = 3
            entry.external_attr = (stat.S_IFREG | (0o755 if relative.endswith(".py") else 0o644)) << 16
            entry.compress_type = zipfile.ZIP_STORED
            archive.writestr(entry, files[relative])


def verify_stage(path: Path, files: dict) -> None:
    if path.is_symlink() or not path.is_dir():
        raise SourceError(f"Existing staging target is not a regular directory: {path}")
    actual = set()
    for item in path.rglob("*"):
        if item.is_symlink() or not (item.is_file() or item.is_dir()):
            raise SourceError(f"Unexpected staging link or special file: {item}")
        if item.is_file():
            relative = item.relative_to(path).as_posix()
            actual.add(relative)
            if relative not in files or read_regular(item) != files[relative]:
                raise SourceError(f"Existing staging content differs; preserved: {item}")
    if actual != set(files):
        raise SourceError(f"Existing staging content is incomplete; preserved: {path}")


def package_sources(cache: Path, destination: Path, output: Path, *, recipe_dir: Path = HERE, repository: Path = REPOSITORY, build_manifests: list[Path] | None = None) -> dict:
    files, manifest = collect_payload(cache, recipe_dir, repository, build_manifests or [])
    verify_payload(files, manifest)
    stage = destination / manifest["top_directory"]
    if output.absolute() == stage.absolute() or stage.absolute() in output.absolute().parents:
        raise SourceError("Output ZIP must be outside the source staging tree")
    if stage.exists() or stage.is_symlink():
        verify_stage(stage, files)
    acquire.ensure_directory(output.parent)
    temporary_zip = None
    temporary_stage = None
    try:
        with tempfile.NamedTemporaryFile(prefix=".nova-source-", suffix=".zip", dir=output.parent, delete=False) as temporary:
            temporary_zip = Path(temporary.name)
        write_zip(temporary_zip, manifest["top_directory"], files)
        verify_zip(temporary_zip)
        if output.exists() or output.is_symlink():
            if output.is_symlink() or not output.is_file() or acquire.sha256_file(output) != acquire.sha256_file(temporary_zip):
                raise SourceError(f"Existing output ZIP differs; preserved: {output}")
        acquire.ensure_directory(destination)
        if not stage.exists():
            temporary_stage = Path(tempfile.mkdtemp(prefix=".nova-source-stage-", dir=destination))
            for relative, data in files.items():
                target = temporary_stage.joinpath(*relative.split("/"))
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
                target.chmod(0o755 if relative.endswith(".py") else 0o644)
            verify_stage(temporary_stage, files)
            temporary_stage.rename(stage)
            temporary_stage = None
        if not output.exists():
            # A hard link atomically publishes without replacing another file.
            output.hardlink_to(temporary_zip)
        return {"output": str(output), "sha256": acquire.sha256_file(output), "bytes": output.stat().st_size, "staging_directory": str(stage), "source_set_id": manifest["source_set_id"], "source_set_sha256": manifest["source_set_sha256"], "recipe_sha256": manifest["recipe_sha256"], "files": len(files), "included_platform_builds": manifest["included_platform_builds"]}
    finally:
        if temporary_zip is not None:
            temporary_zip.unlink(missing_ok=True)
        if temporary_stage is not None:
            shutil.rmtree(temporary_stage)


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, help="Source cache containing archives/; required when creating")
    parser.add_argument("--destination", type=Path, help="Parent of the retained source staging folder; required when creating")
    parser.add_argument("--output", type=Path, required=True, help="Source ZIP to create or verify")
    parser.add_argument("--build-manifest", type=Path, action="append", default=[], help="Optional native build manifest; may be repeated for different platforms")
    parser.add_argument("--verify", action="store_true", help="Verify the existing ZIP without writing")
    parser.add_argument("--expected-sha256", help="Optional separately published ZIP digest to check during --verify")
    args = parser.parse_args(argv)
    if not args.verify and (args.cache is None or args.destination is None):
        parser.error("creation requires --cache and --destination")
    if args.expected_sha256 and not args.verify:
        parser.error("--expected-sha256 requires --verify")
    try:
        if args.verify:
            manifest = verify_zip(args.output, args.expected_sha256)
            print(f"VERIFIED source bundle: {manifest['source_set_id']}; {len(manifest['source_archives'])} original archives; {len(manifest['files']) + 1} files")
        else:
            result = package_sources(args.cache, args.destination, args.output, build_manifests=args.build_manifest)
            print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except (SourceError, OSError, ValueError, KeyError, zipfile.BadZipFile, tarfile.TarError) as exc:
        print(f"SOURCE_BUNDLE_ERROR: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
