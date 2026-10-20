#!/usr/bin/env python3
"""Acquire the exact retained upstream archives in sources.lock.json.

Python 3.9+ and its standard library are sufficient. No source build is run.
The default cache lives beside the repository, outside its tracked files.

    python3 scripts/runtime-source/acquire_sources.py --extract
    python3 scripts/runtime-source/acquire_sources.py --offline --extract
    python3 scripts/runtime-source/acquire_sources.py --offline --verify-only

The source manifest uses paths relative to the cache. A source tree digest is
SHA-256 of UTF-8 JSON (ensure_ascii=False, separators=(',', ':')) containing a
list of {path, sha256} records sorted by relative POSIX path. Only regular files
enter the list; directories carry no content. Permission bits are retained in
the original archives and restored where supported, but excluded from the
content digest so it can be verified on Windows. Links and special files are
rejected.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import sys
import tarfile
import tempfile
import urllib.request


MAX_ARCHIVE_BYTES = 150_000_000
MAX_EXTRACTED_BYTES = 1_000_000_000
TREE_DIGEST_ALGORITHM = "sha256-json-utf8-path-file-sha256-v1"


class SourceError(ValueError):
    """A retained source does not match the pinned source contract."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def safe_path(value: str, *, leaf: bool = False) -> PurePosixPath:
    if not isinstance(value, str) or not value or "\\" in value or "\x00" in value:
        raise SourceError(f"Unsafe source path: {value!r}")
    value = value.rstrip("/")
    parts = value.split("/")
    if any(part in ("", ".", "..") or ":" in part or part.endswith((".", " ")) for part in parts):
        raise SourceError(f"Unsafe source path: {value!r}")
    if leaf and len(parts) != 1:
        raise SourceError(f"Expected a single path component: {value!r}")
    return PurePosixPath(value)


def load_lock(path: Path) -> dict:
    lock = json.loads(path.read_text(encoding="utf-8"))
    if lock.get("schema_version") != 1 or not isinstance(lock.get("sources"), list):
        raise SourceError("Unsupported source lock schema")
    names, archives, source_dirs = set(), set(), set()
    for source in lock["sources"]:
        for key, seen in (("name", names), ("archive", archives), ("source_dir", source_dirs)):
            value = source[key]
            safe_path(value, leaf=True)
            if value in seen:
                raise SourceError(f"Duplicate {key}: {value}")
            seen.add(value)
        safe_path(source["archive_root"], leaf=True)
        if not re.fullmatch(r"[0-9a-f]{64}", source["sha256"]):
            raise SourceError(f"Invalid archive digest: {source['name']}")
        if not isinstance(source["archive_bytes"], int) or not 0 < source["archive_bytes"] <= MAX_ARCHIVE_BYTES:
            raise SourceError(f"Invalid archive size: {source['name']}")
        if not source["url"].startswith("https://"):
            raise SourceError(f"Source URL must use HTTPS: {source['name']}")
        for item in source["license"]["file_hashes"]:
            safe_path(item["path"])
            if not re.fullmatch(r"[0-9a-f]{64}", item["sha256"]):
                raise SourceError(f"Invalid license digest: {source['name']}")
    if sum(source["archive_bytes"] for source in lock["sources"]) > MAX_ARCHIVE_BYTES:
        raise SourceError("Source set exceeds the 150 MB acquisition budget")
    return lock


def ensure_directory(path: Path) -> None:
    # Refuse to follow pre-existing links in any cache-controlled path component.
    for component in (path, *path.parents):
        if component.is_symlink():
            raise SourceError(f"Cache path contains a symlink: {component}")
    path.mkdir(parents=True, exist_ok=True)


def verify_archive(path: Path, source: dict) -> None:
    if path.is_symlink() or not path.is_file():
        raise SourceError(f"Missing regular source archive: {path}")
    if path.stat().st_size != source["archive_bytes"] or sha256_file(path) != source["sha256"]:
        raise SourceError(f"Archive integrity mismatch; existing file was preserved: {path}")


class HTTPSRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if not newurl.startswith("https://"):
            raise SourceError("Refusing a source download redirect outside HTTPS")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def acquire_archive(path: Path, source: dict, *, offline: bool) -> None:
    if path.exists() or path.is_symlink():
        verify_archive(path, source)
        return
    if offline:
        raise SourceError(f"Archive missing in offline mode: {path}")
    ensure_directory(path.parent)
    request = urllib.request.Request(source["url"], headers={"User-Agent": "Nova-runtime-source/1"})
    opener = urllib.request.build_opener(HTTPSRedirectHandler())
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(prefix=f".{source['name']}-", suffix=".part", dir=path.parent, delete=False) as output:
            temporary = Path(output.name)
            with opener.open(request, timeout=90) as response:
                if not response.geturl().startswith("https://"):
                    raise SourceError("Refusing a non-HTTPS source response")
                total = 0
                while True:
                    chunk = response.read(min(1024 * 1024, source["archive_bytes"] + 1 - total))
                    if not chunk:
                        break
                    total += len(chunk)
                    if total > source["archive_bytes"]:
                        raise SourceError(f"Download exceeds pinned size: {source['name']}")
                    output.write(chunk)
        verify_archive(temporary, source)
        if path.exists() or path.is_symlink():
            verify_archive(path, source)
        else:
            # Hard-link creation is atomic and cannot replace another acquisition.
            try:
                os.link(temporary, path)
            except FileExistsError:
                verify_archive(path, source)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def inspected_members(archive: tarfile.TarFile, source: dict) -> list:
    result, seen, total = [], set(), 0
    for member in archive.getmembers():
        path = safe_path(member.name)
        if path.parts[0] != source["archive_root"]:
            raise SourceError(f"Archive has an unexpected root: {member.name}")
        if path.as_posix() in seen:
            raise SourceError(f"Archive has duplicate paths: {member.name}")
        seen.add(path.as_posix())
        if not member.isdir() and not member.isfile():
            raise SourceError(f"Archive links and special files are unsupported: {member.name}")
        if len(path.parts) == 1 and not member.isdir():
            raise SourceError("Archive root must be a directory")
        total += member.size
        if total > MAX_EXTRACTED_BYTES or len(seen) > 100_000:
            raise SourceError("Source archive exceeds extraction limits")
        result.append((member, PurePosixPath(*path.parts[1:])))
    return result


def digest_records(records: list) -> str:
    serialized = json.dumps(sorted(records, key=lambda item: item["path"]), ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(serialized.encode("utf-8")).hexdigest()


def tree_records(path: Path) -> list:
    result = []
    for item in path.rglob("*"):
        if item.is_symlink() or not (item.is_file() or item.is_dir()):
            raise SourceError(f"Source tree contains a link or special file: {item}")
        if item.is_file():
            result.append({"path": item.relative_to(path).as_posix(), "sha256": sha256_file(item)})
    return result


def inspect_archive(path: Path, source: dict) -> tuple:
    records, licenses = [], {}
    expected_licenses = {item["path"]: item for item in source["license"]["file_hashes"]}
    with tarfile.open(path, "r:*") as archive:
        for member, relative in inspected_members(archive, source):
            if member.isfile():
                with archive.extractfile(member) as stream:
                    digest = hashlib.sha256()
                    license_data = [] if relative.as_posix() in expected_licenses else None
                    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                        digest.update(chunk)
                        if license_data is not None:
                            license_data.append(chunk)
                records.append({"path": relative.as_posix(), "sha256": digest.hexdigest()})
                if license_data is not None:
                    data = b"".join(license_data)
                    expected = expected_licenses[relative.as_posix()]
                    if digest.hexdigest() != expected["sha256"] or len(data) != expected["bytes"]:
                        raise SourceError(f"License integrity mismatch: {source['name']}/{relative}")
                    licenses[relative.as_posix()] = data
    if set(licenses) != set(expected_licenses):
        raise SourceError(f"Pinned license text missing: {source['name']}")
    return records, licenses


def extract_archive(path: Path, source: dict, destination: Path, expected_tree_digest: str) -> None:
    if destination.exists() or destination.is_symlink():
        if destination.is_symlink() or not destination.is_dir() or digest_records(tree_records(destination)) != expected_tree_digest:
            raise SourceError(f"Existing source tree differs; it was preserved: {destination}")
        return
    ensure_directory(destination.parent)
    temporary = Path(tempfile.mkdtemp(prefix=f".{source['name']}-", dir=destination.parent))
    try:
        with tarfile.open(path, "r:*") as archive:
            for member, relative in inspected_members(archive, source):
                if not relative.parts:
                    continue
                output = temporary.joinpath(*relative.parts)
                if member.isdir():
                    ensure_directory(output)
                else:
                    ensure_directory(output.parent)
                    with archive.extractfile(member) as stream, output.open("xb") as target:
                        shutil.copyfileobj(stream, target)
                    output.chmod(member.mode & 0o777)
        if digest_records(tree_records(temporary)) != expected_tree_digest:
            raise SourceError(f"Extracted source tree does not match archive: {source['name']}")
        temporary.rename(destination)
    finally:
        if temporary.exists():
            shutil.rmtree(temporary)


def acquire_sources(lock_path: Path, cache: Path, *, offline=False, extract=False, verify_only=False) -> dict:
    lock = load_lock(lock_path)
    manifest = {"schema_version": 1, "source_set_id": lock["source_set_id"], "lock_sha256": sha256_file(lock_path), "tree_digest_algorithm": TREE_DIGEST_ALGORITHM, "patches": lock.get("patches", []), "sources": []}
    for source in lock["sources"]:
        archive_path = cache / "archives" / source["archive"]
        if verify_only:
            verify_archive(archive_path, source)
        else:
            acquire_archive(archive_path, source, offline=offline)
        records, licenses = inspect_archive(archive_path, source)
        tree_digest = digest_records(records)
        entry = dict(source, archive_path=f"archives/{source['archive']}", source_tree_sha256=tree_digest, source_file_count=len(records))
        if extract and not verify_only:
            extract_archive(archive_path, source, cache / "extracted" / source["source_dir"], tree_digest)
            entry["extracted_path"] = f"extracted/{source['source_dir']}"
        if not verify_only:
            for relative, data in licenses.items():
                target = cache / "licenses" / source["name"] / relative
                ensure_directory(target.parent)
                if target.exists() or target.is_symlink():
                    if target.is_symlink() or not target.is_file() or target.read_bytes() != data:
                        raise SourceError(f"Existing license text differs; it was preserved: {target}")
                else:
                    with target.open("xb") as stream:
                        stream.write(data)
            entry["license_directory"] = f"licenses/{source['name']}"
        manifest["sources"].append(entry)
    if not verify_only:
        ensure_directory(cache)
        target = cache / "source-manifest.json"
        if target.is_symlink():
            raise SourceError(f"Manifest path is a symlink: {target}")
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(prefix=".source-manifest-", suffix=".tmp", mode="w", encoding="utf-8", dir=cache, delete=False) as stream:
                temporary = Path(stream.name)
                stream.write(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
            temporary.replace(target)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
    return manifest


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--lock", type=Path, default=Path(__file__).with_name("sources.lock.json"))
    parser.add_argument("--cache-dir", type=Path, default=Path(__file__).resolve().parents[2].parent / "runtime-source-cache")
    parser.add_argument("--offline", action="store_true", help="Require every archive to be cached; do not access the network")
    parser.add_argument("--extract", action="store_true", help="Extract verified sources, refusing to overwrite changed source trees")
    parser.add_argument("--verify-only", action="store_true", help="Read and validate all cached archives and license hashes without writing")
    args = parser.parse_args(argv)
    try:
        manifest = acquire_sources(args.lock, args.cache_dir.absolute(), offline=args.offline, extract=args.extract, verify_only=args.verify_only)
    except (SourceError, OSError, ValueError, tarfile.TarError) as exc:
        print(f"SOURCE_ERROR: {exc}", file=sys.stderr)
        return 1
    total = sum(source["archive_bytes"] for source in manifest["sources"])
    print(f"VERIFIED {len(manifest['sources'])} source archives, {total} bytes; source set {manifest['source_set_id']}")
    if not args.verify_only:
        print(f"Manifest: {args.cache_dir.absolute() / 'source-manifest.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
