"""Public evidence must not leak raw logs/paths or approve mismatched artifacts."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path
import tempfile
import tomllib
import unittest

import public_release_evidence
from test_validation_runner import png


class PublicReleaseEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name)
        self.version = tomllib.loads((Path(__file__).resolve().parent.parent / "Cargo.toml").read_text())["package"]["version"]
        self.platform = "linux-x86_64"
        self.native, self.extracted, self.packages, self.output = [self.base / name for name in ("native", "extracted", "packages", "public")]
        for directory in (self.native, self.extracted, self.packages):
            directory.mkdir()
        self.source = self.base / f"Nova-Flux-{self.version}-FFmpeg-sources.zip"
        self.source.write_bytes(b"sources")
        self.archive = self.packages / f"Nova-Flux-{self.version}-{self.platform}.tar.gz"
        self.archive.write_bytes(b"archive")
        self.stage = self.packages / f"Nova-Flux-{self.version}-{self.platform}"
        self.stage.mkdir()
        self.manifest = {"version": self.version, "platform": self.platform, "distribution_status": "verified-source-runtime-package", "source_bundle": {"filename": self.source.name, "sha256": hashlib.sha256(b"sources").hexdigest(), "builder_sha256": "b" * 64, "sources_lock_sha256": "c" * 64, "source_set_id": "nova-fixture"}, "input_application_sha256": "d" * 64, "packaged_application_sha256": "e" * 64}
        self.write(self.stage / "package-manifest.json", self.manifest)
        self.gui = {"status": "PASS", "frames": 4, "completion": {"screenshot": "/private/home/secret.png", "adapter": "secret"}, "image": {"width": 640, "height": 480, "bytes": len(png()), "sha256": hashlib.sha256(png()).hexdigest()}}
        self.report = {"status": "PASS", "platform": self.platform, "app_version": self.version, "commit": "a" * 40, "host": {"os": "Linux", "architecture": "x86_64", "python": "3.12.1"}, "tests": {"executables": 1, "passed": 3, "failed": 0, "ignored": 0}, "native-smoke": self.gui, "diagnose": {"ready": True, "app_version": self.version, "ffmpeg": "/private/home/ffmpeg"}, "steps": [{"name": "native-smoke", "status": "PASS", "command": ["/private/home/exe"], "error": "secret", "duration_seconds": 1.0}]}
        self.write(self.native / "report.json", self.report)
        self.check = {"status": "PASS", "platform": self.platform, "version": self.version, "archive": self.archive.name, "archive_sha256": hashlib.sha256(b"archive").hexdigest(), "binary_sha256": "e" * 64, "verified_files": 3, "native_gui": self.gui, "diagnose": {"ready": True, "app_version": self.version}}
        self.write(self.extracted / "package-check.json", self.check)
        (self.native / "native-smoke.png").write_bytes(png())
        (self.extracted / "extracted-smoke.png").write_bytes(png())
        (self.native / "raw.log").write_text("secret /private/home")

    def tearDown(self):
        self.temporary.cleanup()

    def write(self, path, value):
        path.write_text(json.dumps(value), encoding="utf-8")

    def export(self):
        return public_release_evidence.export_evidence(self.platform, self.native, self.extracted, self.packages, self.source, self.output)

    def test_public_payload_excludes_raw_paths_logs_and_unknown_diagnostic_fields(self):
        receipt = self.export()
        self.assertEqual(receipt["status"], "PASS")
        self.assertEqual(receipt["archive"]["sha256"], hashlib.sha256(b"archive").hexdigest())
        self.assertEqual({p.name for p in self.output.iterdir()}, {"validation.json", "receipt.json", "native-smoke.png", "extracted-smoke.png"})
        text = (self.output / "validation.json").read_text() + (self.output / "receipt.json").read_text()
        self.assertNotIn("secret", text)
        self.assertNotIn("/private/home", text)
        self.assertNotIn("command", text)

    def test_mismatched_source_hash_is_never_approved(self):
        self.manifest["source_bundle"]["sha256"] = "0" * 64
        self.write(self.stage / "package-manifest.json", self.manifest)
        with self.assertRaises(ValueError):
            self.export()

    def test_extracted_acceptance_must_describe_this_archive(self):
        self.check["archive_sha256"] = "0" * 64
        self.write(self.extracted / "package-check.json", self.check)
        with self.assertRaises(ValueError):
            self.export()

    def test_missing_acceptance_produces_incomplete_evidence(self):
        (self.extracted / "package-check.json").unlink()
        receipt = self.export()
        self.assertEqual(receipt["status"], "INCOMPLETE")
        self.assertEqual(json.loads((self.output / "validation.json").read_text())["extracted"]["status"], "NOT_RUN")

    def test_failed_or_ignored_tests_cannot_approve_a_package(self):
        self.report["tests"]["ignored"] = 1
        self.write(self.native / "report.json", self.report)
        with self.assertRaises(ValueError):
            self.export()


if __name__ == "__main__":
    unittest.main()
