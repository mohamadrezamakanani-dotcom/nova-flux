#!/usr/bin/env python3
"""Test the copied runtime with Homebrew, original apps, and networking denied."""
import json
import os
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent
RUNTIME = ROOT / "runtime"
FIXTURES = ROOT / "smoke"
FIXTURES.mkdir(exist_ok=True)
PROFILE = '(version 1)(allow default)(deny file-read* (subpath "/opt/homebrew") (subpath "/opt/local") (subpath "/Applications"))(deny network*)'
ENV = {"PATH": "/usr/bin:/bin", "TMPDIR": str(FIXTURES)}
REPORT = {"isolation": "sandbox denies /Applications, /opt/homebrew, /opt/local, and all networking; environment cleared", "cases": []}

def run(binary, *args):
    command = ["/usr/bin/sandbox-exec", "-p", PROFILE, str(RUNTIME / "bin" / binary), *args]
    p = subprocess.run(command, env=ENV, cwd=FIXTURES, capture_output=True, text=True, timeout=90)
    if p.returncode:
        raise RuntimeError(f"Command failed ({p.returncode}): {command}\n{p.stderr}")
    return p.stdout

manifest = json.loads((RUNTIME / "manifest.json").read_text())
for item in manifest["files"]:
    p = RUNTIME / item["path"]
    assert subprocess.check_output(["/usr/bin/lipo", "-archs", str(p)], text=True).strip() == "arm64"
    for dep in subprocess.check_output(["/usr/bin/otool", "-L", str(p)], text=True).splitlines()[1:]:
        dep = dep.strip().split(" (")[0]
        assert dep.startswith(("/usr/lib/", "/System/", "@loader_path/")), (p, dep)
        if dep.startswith("@loader_path/"):
            assert (p.parent / dep.removeprefix("@loader_path/")).resolve().is_file(), (p, dep)
    subprocess.run(["/usr/bin/codesign", "--verify", "--strict", str(p)], check=True)
REPORT["architecture_dependency_signature_audits"] = len(manifest["files"])
run("ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=193x129:rate=12:duration=1.25", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=1.25", "-c:v", "libx264", "-pix_fmt", "yuv444p", "-c:a", "aac", "-shortest", "odd-input.mp4")

cases = [
    ("video.mp4", ["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"], {"h264", "aac"}),
    ("video.mov", ["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"], {"h264", "aac"}),
    ("video.mkv", ["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"], {"h264", "aac"}),
    ("video.webm", ["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-c:a", "libopus"], {"vp9", "opus"}),
    ("video.avi", ["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "mpeg4", "-c:a", "libmp3lame"], {"mpeg4", "mp3"}),
    ("audio.mp3", ["-vn", "-c:a", "libmp3lame"], {"mp3"}),
    ("audio.m4a", ["-vn", "-c:a", "aac"], {"aac"}),
    ("audio.aac", ["-vn", "-c:a", "aac"], {"aac"}),
    ("audio.ogg", ["-vn", "-c:a", "libvorbis"], {"vorbis"}),
    ("audio.flac", ["-vn", "-c:a", "flac"], {"flac"}),
    ("audio.wav", ["-vn", "-c:a", "pcm_s16le"], {"pcm_s16le"}),
    ("animated.gif", ["-an", "-vf", "fps=8,scale=128:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse", "-loop", "0"], {"gif"}),
]
for name, options, expected in cases:
    run("ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i", "odd-input.mp4", *options, name)
    info = json.loads(run("ffprobe", "-v", "error", "-show_streams", "-show_format", "-of", "json", name))
    actual = {s["codec_name"] for s in info["streams"]}
    assert expected == actual, (name, actual, expected)
    assert float(info["format"]["duration"]) > 0.5
    run("ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-i", name, "-f", "null", "-")
    REPORT["cases"].append({"file": name, "codecs": sorted(actual), "bytes": (FIXTURES / name).stat().st_size, "decoded": True})
    print("PASS", name, sorted(actual), flush=True)

# Log exactly which libraries dyld loads from the relocated executable.
p = subprocess.run([str(RUNTIME / "bin/ffmpeg"), "-hide_banner", "-version"], env={**ENV, "DYLD_PRINT_LIBRARIES": "1"}, text=True, capture_output=True, check=True)
(ROOT / "dyld-loaded-libraries.log").write_text(p.stderr)
assert "/Applications/" not in p.stderr and "/opt/homebrew/" not in p.stderr and "/opt/local/" not in p.stderr
REPORT["dyld_load_audit"] = "No /Applications, /opt/homebrew, or /opt/local libraries loaded"
(ROOT / "verification.json").write_text(json.dumps(REPORT, indent=2) + "\n")
print(json.dumps({"passed": len(REPORT["cases"]), "audited_binaries": REPORT["architecture_dependency_signature_audits"]}))
