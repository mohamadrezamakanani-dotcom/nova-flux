#!/usr/bin/env python3
"""Package only copies of the existing Shotcut FFmpeg arm64 dependency closure."""
import hashlib
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent
OUT = ROOT / "runtime"
SOURCES = json.loads((ROOT / "shotcut-closure.json").read_text())

def run(*args):
    return subprocess.run(args, check=True, text=True, capture_output=True).stdout

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def deps(path):
    return [line.strip().split(" (")[0] for line in run("/usr/bin/otool", "-L", str(path)).splitlines()[1:]]

for d in [OUT / "bin", OUT / "lib", OUT / "licenses"]:
    d.mkdir(parents=True, exist_ok=True)

items = []
for original in SOURCES:
    src = Path(original)
    is_binary = src.name in ["ffmpeg", "ffprobe"]
    target = OUT / ("bin" if is_binary else "lib") / src.name
    run("/usr/bin/lipo", str(src), "-thin", "arm64", "-output", str(target))
    target.chmod(0o755)
    run("/usr/bin/codesign", "--remove-signature", str(target))
    if not is_binary:
        run("/usr/bin/install_name_tool", "-id", "@loader_path/" + src.name, str(target))
    for dependency in deps(target):
        if dependency.startswith(("/usr/lib/", "/System/")):
            continue
        name = Path(dependency).name
        if name == src.name:
            continue
        resolved = OUT / "lib" / name
        if not any(Path(p).name == name for p in SOURCES):
            raise RuntimeError(f"Dependency missing from closure: {dependency}")
        replacement = ("@loader_path/../lib/" if is_binary else "@loader_path/") + name
        run("/usr/bin/install_name_tool", "-change", dependency, replacement, str(target))
    load_commands = run("/usr/bin/otool", "-l", str(target))
    for rpath in re.findall(r"cmd LC_RPATH\s+cmdsize \d+\s+path (.*?) \(offset", load_commands):
        run("/usr/bin/install_name_tool", "-delete_rpath", rpath, str(target))
    run("/usr/bin/codesign", "--force", "--sign", "-", "--timestamp=none", str(target))
    run("/usr/bin/codesign", "--verify", "--strict", str(target))
    items.append({"path": str(target.relative_to(OUT)), "bytes": target.stat().st_size,
                  "sha256": sha(target), "source_filename": src.name,
                  "source_sha256": sha(src), "dependencies": deps(target)})

manifest = {"provider": "Shotcut", "provider_url": "https://shotcut.org/",
            "source": "Already installed Shotcut.app; originals unmodified",
            "ffmpeg_version": run(str(OUT / "bin/ffmpeg"), "-hide_banner", "-version"),
            "architecture": "arm64", "license": "GPL-3.0-or-later (FFmpeg build; see licenses for dependencies)",
            "files": items, "total_binary_bytes": sum(x["bytes"] for x in items)}
(OUT / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
print(json.dumps({"runtime": str(OUT), "files": len(items), "bytes": manifest["total_binary_bytes"]}, indent=2))
