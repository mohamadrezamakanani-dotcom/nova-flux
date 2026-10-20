"""Fail-closed checks for native-render evidence, without opening an application."""
from __future__ import annotations

import json
from contextlib import redirect_stdout
import io
import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest.mock import patch
import zlib

from run_validation import Validation, png_info, validate_smoke
import run_validation


class NativeSourceHostTests(unittest.TestCase):
    def test_macos_source_validation_requires_an_existing_native_runtime(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = Path(temporary)
            with patch("run_validation.platform.system", return_value="Darwin"), patch("run_validation.platform.machine", return_value="arm64"):
                run_validation.require_validation_host("macos-arm64", source_only=True, runtime=runtime)
                with self.assertRaises(ValueError):
                    run_validation.require_validation_host("macos-arm64", source_only=True, runtime=runtime / "missing")

    def test_macos_cannot_enter_the_legacy_private_packager_or_an_intel_host(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch("run_validation.platform.system", return_value="Darwin"), patch("run_validation.platform.machine", return_value="arm64"):
                with self.assertRaises(ValueError):
                    run_validation.require_validation_host("macos-arm64", source_only=False, runtime=Path(temporary))
            with patch("run_validation.platform.system", return_value="Darwin"), patch("run_validation.platform.machine", return_value="x86_64"):
                with self.assertRaises(ValueError):
                    run_validation.require_validation_host("macos-arm64", source_only=True, runtime=Path(temporary))


def png(width: int = 640, height: int = 480) -> bytes:
    def chunk(kind: bytes, payload: bytes) -> bytes:
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))
    pixels = (b"\x00" + b"\x31\x62\x93\xff" * width) * height
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(pixels)) + chunk(b"IEND", b"")


class RenderEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.screenshot = Path(self.temporary.name) / "native.png"
        self.screenshot.write_bytes(png())
        self.event = {"event": "smoke_complete", "app_version": "2.3.0", "platform": "windows", "frames": 4, "screenshot": str(self.screenshot), "width": 640, "height": 480}

    def tearDown(self):
        self.temporary.cleanup()

    def events(self) -> str:
        return json.dumps({"event": "smoke_started", "adapter": "test fixture only"}) + "\n" + json.dumps(self.event) + "\n"

    def test_accepts_consistent_frame_version_and_complete_png_evidence(self):
        result = validate_smoke(self.events(), self.screenshot, "2.3.0", "windows-x86_64")
        self.assertEqual(result["frames"], 4)
        self.assertEqual(png_info(self.screenshot)["width"], 640)

    def test_missing_complete_event_is_never_reported_as_a_native_pass(self):
        with self.assertRaises(ValueError):
            validate_smoke('{"event":"smoke_started"}\n', self.screenshot, "2.3.0", "windows-x86_64")

    def test_stale_binary_wrong_host_and_insufficient_frames_are_rejected(self):
        for key, value in (("app_version", "2.2.0"), ("platform", "linux"), ("frames", 2)):
            original = self.event[key]
            self.event[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate_smoke(self.events(), self.screenshot, "2.3.0", "windows-x86_64")
            self.event[key] = original

    def test_png_must_match_the_requested_path_and_reported_dimensions(self):
        for key, value in (("screenshot", str(self.screenshot.parent / "other.png")), ("width", 641)):
            original = self.event[key]
            self.event[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate_smoke(self.events(), self.screenshot, "2.3.0", "windows-x86_64")
            self.event[key] = original

    def test_truncated_or_corrupt_png_is_not_accepted_from_its_header_alone(self):
        for content in (png()[:40], png()[:-12], png()[:-1] + b"\x00"):
            self.screenshot.write_bytes(content)
            with self.assertRaises(ValueError):
                png_info(self.screenshot)

    def test_nonzero_native_command_has_a_failed_json_record_and_complete_log(self):
        directory = self.screenshot.parent
        validation = Validation(directory, directory, "windows-x86_64", "2.3.0")
        with redirect_stdout(io.StringIO()), self.assertRaisesRegex(RuntimeError, "exit code 7"):
            validation.step("intentional-failure", [sys.executable, "-c", "print('command output'); raise SystemExit(7)"], environment=os.environ.copy(), timeout=5)
        report = json.loads((directory / "report.json").read_text())
        self.assertEqual(report["steps"][0]["status"], "FAIL")
        self.assertEqual(report["steps"][0]["exit_code"], 7)
        self.assertIn("command output", (directory / report["steps"][0]["log"]).read_text())

    def test_timeout_terminates_its_child_and_records_the_deadline_failure(self):
        directory = self.screenshot.parent
        validation = Validation(directory, directory, "linux-x86_64", "2.3.0")
        with redirect_stdout(io.StringIO()), self.assertRaisesRegex(RuntimeError, "deadline"):
            validation.step("intentional-timeout", [sys.executable, "-c", "import time; time.sleep(20)"], environment=os.environ.copy(), timeout=0.1)
        report = json.loads((directory / "report.json").read_text())
        self.assertTrue(report["steps"][0]["timed_out"])
        self.assertEqual(report["steps"][0]["status"], "FAIL")


if __name__ == "__main__":
    unittest.main()
