#!/usr/bin/env python3
"""Native Windows/Linux private CI validation with durable JSON and log evidence."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import signal
import struct
import subprocess
import sys
import threading
import time
import zlib

from portable_common import PLATFORMS, require_host, sha256


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def png_info(path: Path) -> dict:
    if not path.is_file() or not 40 <= path.stat().st_size <= 32 * 1024 * 1024:
        raise ValueError("Native screenshot is missing or has an invalid file length")
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("Native screenshot is not a PNG")
    offset, width, height, channels = 8, 0, 0, 0
    compressed = bytearray()
    ended = False
    while offset + 12 <= len(data):
        length = struct.unpack_from(">I", data, offset)[0]
        end = offset + 12 + length
        if end > len(data):
            raise ValueError("Native screenshot has a truncated PNG chunk")
        kind, payload = data[offset + 4:offset + 8], data[offset + 8:end - 4]
        expected_crc = struct.unpack_from(">I", data, end - 4)[0]
        if zlib.crc32(kind + payload) != expected_crc:
            raise ValueError("Native screenshot PNG failed its chunk CRC")
        if kind == b"IHDR":
            if offset != 8 or length != 13:
                raise ValueError("Native screenshot PNG has an invalid header")
            width, height, depth, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", payload)
            if not 640 <= width <= 8192 or not 480 <= height <= 8192 or width * height > 32_000_000:
                raise ValueError("Native screenshot has no usable desktop-sized image")
            if depth != 8 or color not in (2, 6) or (compression, filtering, interlace) != (0, 0, 0):
                raise ValueError("Expected a complete non-interlaced RGB/RGBA8 screenshot")
            channels = 3 if color == 2 else 4
        elif kind == b"IDAT":
            compressed.extend(payload)
        elif kind == b"IEND":
            if payload or end != len(data):
                raise ValueError("Native screenshot has invalid PNG end data")
            ended = True
            break
        offset = end
    if not ended or not channels or not compressed:
        raise ValueError("Native screenshot PNG is incomplete")
    row_bytes = width * channels + 1
    expected = row_bytes * height
    decoder = zlib.decompressobj()
    try:
        pixels = decoder.decompress(compressed, expected + 1)
    except zlib.error as error:
        raise ValueError("Native screenshot pixel data is corrupt") from error
    if len(pixels) != expected or not decoder.eof or decoder.unused_data or decoder.unconsumed_tail:
        raise ValueError("Native screenshot pixel data is incomplete or exceeds its dimensions")
    if any(pixels[index] > 4 for index in range(0, expected, row_bytes)):
        raise ValueError("Native screenshot has invalid PNG scanline filters")
    return {"width": width, "height": height, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def validate_smoke(output: str, screenshot: Path, version: str, target: str) -> dict:
    events = []
    for line in output.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(event, dict) and isinstance(event.get("event"), str):
            events.append(event)
    started = [event for event in events if event["event"] == "smoke_started"]
    completed = [event for event in events if event["event"] == "smoke_complete"]
    if len(started) != 1 or len(completed) != 1 or any(event["event"] == "smoke_failed" for event in events):
        raise ValueError("Native GUI did not emit exactly one start and successful completion event")
    result = completed[0]
    if result.get("app_version") != version or result.get("platform") != target.split("-")[0]:
        raise ValueError("Native smoke event belongs to a stale application or wrong operating system")
    if type(result.get("frames")) is not int or result["frames"] < 3:
        raise ValueError("Native GUI did not render at least three frames")
    if not isinstance(result.get("screenshot"), str) or Path(result["screenshot"]).resolve() != screenshot.resolve():
        raise ValueError("Native smoke completed without the requested screenshot path")
    image = png_info(screenshot)
    if (result.get("width"), result.get("height")) != (image["width"], image["height"]):
        raise ValueError("Native screenshot dimensions differ from the renderer completion event")
    return {"status": "PASS", "frames": result["frames"], "started": started[0], "completion": result, "image": image}


def stop_process_tree(process: subprocess.Popen) -> None:
    if os.name == "nt":
        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


class Validation:
    def __init__(self, root: Path, directory: Path, target: str, version: str):
        self.root, self.directory = root, directory
        self.report = {
            "schema": 1, "status": "RUNNING", "started_at": utc_now(),
            "platform": target, "app_version": version,
            "host": {"os": platform.system(), "architecture": platform.machine(), "python": platform.python_version()},
            "commit": os.environ.get("GITHUB_SHA"), "run_id": os.environ.get("GITHUB_RUN_ID"),
            "distribution_status": "private-ci-only", "steps": [],
        }

    def save(self):
        (self.directory / "report.json").write_text(json.dumps(self.report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    def step(self, name: str, command: list[str], *, environment: dict[str, str], timeout: int) -> str:
        log = self.directory / f"{len(self.report['steps']) + 1:02d}-{name}.log"
        entry = {"name": name, "command": command, "log": log.name, "started_at": utc_now(), "status": "RUNNING"}
        self.report["steps"].append(entry)
        self.save()
        print(json.dumps({"event": "validation_step_started", "name": name, "command": command}), flush=True)
        started = time.monotonic()
        captured, reader_errors = [], []
        try:
            options = {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt" else {"start_new_session": True}
            with log.open("wb") as destination:
                process = subprocess.Popen(command, cwd=self.root, env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, **options)

                def read_output():
                    retained = 0
                    try:
                        for line in iter(process.stdout.readline, b""):
                            destination.write(line)
                            destination.flush()
                            print(line.decode("utf-8", errors="replace").rstrip("\r\n"), flush=True)
                            if retained < 16 * 1024 * 1024:
                                captured.append(line)
                                retained += len(line)
                    except Exception as error:
                        reader_errors.append(str(error))

                reader = threading.Thread(target=read_output, daemon=True)
                reader.start()
                try:
                    entry["exit_code"] = process.wait(timeout=timeout)
                except subprocess.TimeoutExpired:
                    entry["timed_out"] = True
                    stop_process_tree(process)
                    process.wait(timeout=30)
                    raise RuntimeError(f"{name} exceeded its {timeout}-second deadline")
                finally:
                    reader.join(timeout=5)
                    if reader.is_alive():
                        stop_process_tree(process)
                        reader.join(timeout=5)
                    process.stdout.close()
                if reader.is_alive() or reader_errors:
                    raise RuntimeError(f"{name} output could not be collected completely: {reader_errors}")
            if entry["exit_code"] != 0:
                raise RuntimeError(f"{name} failed with exit code {entry['exit_code']}; see {log.name}")
            entry["status"] = "PASS"
            return b"".join(captured).decode("utf-8", errors="replace")
        except Exception as error:
            entry.update(status="FAIL", error=str(error))
            raise
        finally:
            entry.update(completed_at=utc_now(), duration_seconds=round(time.monotonic() - started, 3))
            self.save()


def validate_diagnosis(output: str, version: str) -> dict:
    info = json.loads(output)
    if info.get("ready") is not True or info.get("app_version") != version or not info.get("version"):
        raise ValueError("Runtime diagnosis is not ready or belongs to a stale application")
    return info


def private_repository_guard(source_only: bool = False) -> None:
    if os.environ.get("GITHUB_ACTIONS") == "true":
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
        if not source_only and event.get("repository", {}).get("private") is not True:
            raise ValueError("Runtime acquisition and binary artifacts are restricted to a private CI repository")


def main() -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=PLATFORMS, required=True)
    parser.add_argument("--runtime", type=Path, default=Path("runtime"))
    parser.add_argument("--report-dir", type=Path, required=True)
    parser.add_argument("--package-destination", type=Path, default=Path("dist"))
    parser.add_argument("--cache", type=Path, default=Path(".runtime-downloads"))
    parser.add_argument("--toolchain", default="1.99.0")
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--private-ci", action="store_true", help="Explicitly authorize local private validation packages only")
    mode.add_argument("--source-only", action="store_true", help="Run every source/media/native-GUI check without packaging or uploading binaries")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    directory = args.report_dir.absolute()
    if directory.exists():
        raise ValueError("Use a fresh report directory so old screenshots cannot satisfy a new run")
    directory.mkdir(parents=True)
    version_match = re.search(r'^version\s*=\s*"([^"]+)"', (root / "Cargo.toml").read_text(encoding="utf-8"), re.M)
    if not version_match:
        raise ValueError("Cannot determine application version")
    version = version_match.group(1)
    validation = Validation(root, directory, args.platform, version)
    if args.source_only:
        validation.report["distribution_status"] = "source-only-ci-no-binary-artifacts"
    validation.save()
    try:
        private_repository_guard(args.source_only)
        require_host(args.platform)
        runtime = args.runtime.absolute()
        environment = os.environ.copy()
        environment.update({
            "NOVA_RUNTIME": str(runtime), "NOVA_TEST_RUNTIME": str(runtime),
            "NOVA_DATA_DIR": str(directory / "state"), "NOVA_CACHE_DIR": str(directory / "cache"),
            "CARGO_TERM_COLOR": "never", "RUST_BACKTRACE": "1",
        })
        if args.platform.startswith("linux-"):
            xdg = directory / "xdg-runtime"
            xdg.mkdir(mode=0o700)
            environment["XDG_RUNTIME_DIR"] = str(xdg)
        run = lambda name, command, timeout=1800, env=environment: validation.step(name, command, environment=env, timeout=timeout)
        run("python-boundary-tests", [sys.executable, "-m", "unittest", "discover", "-s", "scripts", "-p", "test_*.py"], 120)
        if not runtime.exists():
            run("fetch-pinned-runtime", [sys.executable, "scripts/fetch_portable_runtime.py", "--platform", args.platform, "--destination", str(runtime), "--cache", str(args.cache.absolute())], 900)
        cargo = ["cargo", f"+{args.toolchain}"]
        run("rust-toolchain", ["rustc", f"+{args.toolchain}", "--version", "--verbose"], 60)
        run("format", [*cargo, "fmt", "--all", "--", "--check"], 120)
        run("clippy", [*cargo, "clippy", "--all-targets", "--locked", "--", "-D", "warnings"], 2400)
        test_output = run("all-real-media-tests", [*cargo, "test", "--all-targets", "--locked", "--", "--include-ignored", "--test-threads=1"], 2400)
        summaries = re.findall(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", test_output)
        if not summaries or any(status != "ok" or int(failed) or int(ignored) for status, _, failed, ignored in summaries):
            raise ValueError("Real-media test run is missing complete passing summaries or left ignored tests")
        validation.report["tests"] = {"executables": len(summaries), "passed": sum(int(passed) for _, passed, _, _ in summaries), "failed": 0, "ignored": 0}
        run("release-build", [*cargo, "build", "--release", "--locked"], 2400)
        binary = root / "target/release" / ("nova-flux.exe" if args.platform.startswith("windows-") else "nova-flux")
        validation.report["diagnose"] = validate_diagnosis(run("release-diagnose", [str(binary), "--diagnose"], 60), version)

        def smoke(name: str, executable: Path, env: dict[str, str]):
            screenshot = directory / f"{name}.png"
            smoke_env = {**env, "NOVA_SMOKE_TEST": "1", "NOVA_SMOKE_TEST_MS": "3000", "NOVA_SMOKE_SCREENSHOT": str(screenshot)}
            output = run(name, [str(executable), "--smoke-test"], 45, smoke_env)
            validation.report[name] = validate_smoke(output, screenshot, version, args.platform)
            validation.save()

        smoke("native-smoke", binary, environment)
        if args.source_only:
            validation.report["status"] = "PASS"
            return 0
        package_output = run("portable-package", [sys.executable, "scripts/package_portable.py", "--platform", args.platform, "--runtime", str(runtime), "--binary", str(binary), "--destination", str(args.package_destination.absolute()), "--channel", "private-ci"], 900)
        package = json.loads(package_output)
        archive = Path(package["archive"])
        archive.relative_to(args.package_destination.absolute())
        if package.get("status") != "PASS" or package.get("distribution_status") != "private-ci-only" or sha256(archive) != package.get("sha256"):
            raise ValueError("Private portable package failed its archive/hash contract")
        name = archive.name.removesuffix(".zip").removesuffix(".tar.gz")
        stage = archive.parent / name
        manifest = json.loads((stage / "package-manifest.json").read_text(encoding="utf-8"))
        if manifest.get("version") != version or manifest.get("platform") != args.platform or manifest.get("distribution_status") != "private-ci-only":
            raise ValueError("Portable package manifest does not match this application and target")
        packaged = stage / binary.name
        if manifest.get("binary_sha256") != sha256(packaged):
            raise ValueError("Staged application hash differs from the package manifest")
        packaged_env = environment.copy()
        packaged_env.pop("NOVA_RUNTIME", None)
        validation.report["packaged_diagnose"] = validate_diagnosis(run("packaged-diagnose", [str(packaged), "--diagnose"], 60, packaged_env), version)
        smoke("packaged-smoke", packaged, packaged_env)
        validation.report["package"] = package
        validation.report["status"] = "PASS"
        return 0
    except Exception as error:
        validation.report.update(status="FAIL", error=str(error))
        print(json.dumps({"event": "validation_failed", "error": str(error)}), flush=True)
        return 1
    finally:
        validation.report["completed_at"] = utc_now()
        validation.save()
        print(json.dumps({"event": "validation_complete", "status": validation.report["status"], "report": str(directory / "report.json")}), flush=True)


if __name__ == "__main__":
    raise SystemExit(main())
