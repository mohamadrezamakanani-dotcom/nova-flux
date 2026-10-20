#!/usr/bin/env python3
"""Shared, standard-library-only validation for private portable CI artifacts."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path, PurePosixPath
import platform
import re
import struct
import subprocess
import tarfile
import zipfile

PLATFORMS = ("windows-x86_64", "linux-x86_64")
REQUIRED_ENCODERS = frozenset((
    "libx264", "mpeg4", "libvpx-vp9", "aac", "libmp3lame", "pcm_s16le",
    "libvorbis", "libopus", "flac",
))
PRIVATE_NOTICE = """PRIVATE CI VALIDATION ARTIFACT — NOT A PUBLIC RELEASE

This archive is for native build, conversion and GUI acceptance testing only.
It includes unmodified GPL-enabled FFmpeg/ffprobe from BtbN/FFmpeg-Builds.
The bundled provider license and documentation remain under their own terms.
The builder project's MIT license is not the license of its FFmpeg binaries.

Exact binary/archive hashes, download URL, FFmpeg revision and builder commit
are recorded in runtime/manifest.json and runtime/provenance/.
Complete corresponding source for linked FFmpeg dependencies has not been
assembled or verified. Those links and hashes do not establish source
completeness. Public redistribution is blocked until that work is complete.

No test media, user preferences or user files are included. The application
does not download a runtime at launch. Its bundled binaries run locally.
"""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def json_write(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def binary_architecture(path: Path) -> str:
    with path.open("rb") as stream:
        header = stream.read(64)
        if header.startswith(b"\x7fELF"):
            if len(header) >= 20 and header[4:6] == b"\x02\x01" and struct.unpack_from("<H", header, 18)[0] == 62:
                return "elf-x86_64"
            raise ValueError(f"Expected a little-endian 64-bit x86_64 ELF: {path.name}")
        if header[:2] == b"MZ" and len(header) == 64:
            stream.seek(struct.unpack_from("<I", header, 60)[0])
            pe = stream.read(26)
            if len(pe) == 26 and pe[:4] == b"PE\0\0" and struct.unpack_from("<H", pe, 4)[0] == 0x8664 and struct.unpack_from("<H", pe, 24)[0] == 0x20B:
                return "pe-x86_64"
            raise ValueError(f"Expected an x86_64 PE32+ binary: {path.name}")
    raise ValueError(f"Unrecognized executable format: {path.name}")


def require_host(target: str) -> None:
    expected = "Windows" if target.startswith("windows-") else "Linux"
    if platform.system() != expected or platform.machine().lower() not in ("x86_64", "amd64"):
        raise ValueError(f"Native {target} host required; this host is {platform.system()} {platform.machine()}")


def run(command: list[str], *, env: dict | None = None, timeout: int = 60) -> str:
    result = subprocess.run(command, check=True, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout, env=env)
    return result.stdout


def runtime_binaries(runtime: Path, target: str) -> list[Path]:
    suffix = ".exe" if target.startswith("windows-") else ""
    return [runtime / "bin" / (name + suffix) for name in ("ffmpeg", "ffprobe")]


def inspect_runtime(runtime: Path, target: str, version_prefix: str) -> dict:
    require_host(target)
    expected = "pe-x86_64" if target.startswith("windows-") else "elf-x86_64"
    ffmpeg, ffprobe = runtime_binaries(runtime, target)
    versions = {}
    for tool in (ffmpeg, ffprobe):
        if binary_architecture(tool) != expected:
            raise ValueError(f"Wrong architecture for {tool.name}")
        output = run([str(tool), "-hide_banner", "-version"])
        if not output.splitlines() or version_prefix not in output.splitlines()[0]:
            raise ValueError(f"Unexpected {tool.name} version")
        if "--enable-nonfree" in output or "--enable-gpl" not in output:
            raise ValueError("Runtime must be the pinned GPL, redistributable variant; nonfree is rejected")
        versions[tool.stem] = output
    available = set(re.findall(r"^\s*[VAS][A-Z.]{5}\s+(\S+)", run([str(ffmpeg), "-hide_banner", "-encoders"]), re.M))
    missing = REQUIRED_ENCODERS - available
    if missing:
        raise ValueError("Required CPU encoders missing: " + ", ".join(sorted(missing)))
    return {"versions": versions, "required_encoders": sorted(REQUIRED_ENCODERS), "native_runtime_check": "PASS"}


def checked_member(name: str, seen: set[str]) -> PurePosixPath:
    # Apply Windows path restrictions on both hosts, so Linux validation cannot
    # approve a ZIP that would escape or alias a different entry on Windows.
    if not name or "\\" in name or "\x00" in name or name.startswith("/"):
        raise ValueError(f"Unsafe archive member: {name!r}")
    raw_parts = name.split("/")
    reserved = {"con", "prn", "aux", "nul", *(f"com{i}" for i in range(1, 10)), *(f"lpt{i}" for i in range(1, 10))}
    path = PurePosixPath(name)
    if path.is_absolute() or any(part in ("", ".", "..") or any(ord(c) < 32 or c in ':<>"|?*' for c in part) or part.endswith((".", " ")) or part.split(".", 1)[0].casefold() in reserved for part in raw_parts):
        raise ValueError(f"Unsafe archive member: {name!r}")
    normalized = str(path).casefold()
    if normalized in seen:
        raise ValueError(f"Duplicate/case-colliding archive member: {name!r}")
    seen.add(normalized)
    return path


def safe_extract(archive: Path, destination: Path, archive_format: str) -> None:
    """Extract regular files only, with bounded sizes and no links/devices."""
    maximum = 3 * 1024**3
    total = 0
    seen: set[str] = set()
    destination.mkdir(parents=True, exist_ok=False)
    if archive_format == "zip":
        with zipfile.ZipFile(archive) as source:
            for member in source.infolist():
                relative = checked_member(member.filename.rstrip("/"), seen)
                mode = (member.external_attr >> 16) & 0o170000
                if mode not in (0, 0o040000, 0o100000):
                    raise ValueError(f"Links and special archive entries are forbidden: {member.filename}")
                if member.flag_bits & 1:
                    raise ValueError("Encrypted archive entries are forbidden")
                total += member.file_size
                if total > maximum:
                    raise ValueError("Runtime archive exceeds the 3 GiB extraction budget")
                target = destination.joinpath(*relative.parts)
                if member.is_dir():
                    target.mkdir(parents=True, exist_ok=True)
                else:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with source.open(member) as inp, target.open("xb") as out:
                        copy_bounded(inp, out, member.file_size)
    elif archive_format == "tar.xz":
        with tarfile.open(archive, "r:xz") as source:
            for member in source:
                relative = checked_member(member.name.rstrip("/"), seen)
                if not (member.isfile() or member.isdir()):
                    raise ValueError(f"Links and special archive entries are forbidden: {member.name}")
                total += member.size
                if total > maximum:
                    raise ValueError("Runtime archive exceeds the 3 GiB extraction budget")
                target = destination.joinpath(*relative.parts)
                if member.isdir():
                    target.mkdir(parents=True, exist_ok=True)
                else:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with source.extractfile(member) as inp, target.open("xb") as out:
                        copy_bounded(inp, out, member.size)
                    target.chmod(0o755 if member.mode & 0o111 else 0o644)
    else:
        raise ValueError(f"Unsupported archive format: {archive_format}")


def copy_bounded(inp, out, expected: int) -> None:
    remaining = expected
    while remaining:
        chunk = inp.read(min(1024 * 1024, remaining))
        if not chunk:
            raise ValueError("Truncated archive entry")
        out.write(chunk)
        remaining -= len(chunk)
    if inp.read(1):
        raise ValueError("Archive entry exceeds its declared size")


def files_manifest(root: Path, exclude: tuple[str, ...] = ()) -> list[dict]:
    files = []
    for item in sorted(root.rglob("*")):
        if item.is_symlink():
            raise ValueError(f"Symlinks are not accepted in portable packages: {item.name}")
        if item.is_file() and item.relative_to(root).as_posix() not in exclude:
            files.append({"path": item.relative_to(root).as_posix(), "bytes": item.stat().st_size, "sha256": sha256(item)})
    return files


def verify_files(root: Path, files: list[dict], *, exclude: tuple[str, ...] = ()) -> None:
    actual = files_manifest(root, exclude)
    if actual != files:
        raise ValueError("Runtime contents do not match their manifest (missing, modified or additional files)")
