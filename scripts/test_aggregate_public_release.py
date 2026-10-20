"""Cross-platform acceptance must bind one source commit and common ZIP."""
from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import aggregate_public_release


class AggregatePublicReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name)
        self.root = self.base / "repo"
        recipe = self.root / "scripts/runtime-source"
        recipe.mkdir(parents=True)
        (self.root / "Cargo.toml").write_text('[package]\nversion = "1.2.3"\n')
        (recipe / "build_runtime.py").write_bytes(b"builder")
        (recipe / "sources.lock.json").write_text('{"source_set_id":"nova-test"}\n')
        self.commit = "a" * 40
        self.source = self.base / "Nova-Flux-1.2.3-FFmpeg-sources.zip"
        self.source.write_bytes(b"sources")
        self.binding = {"filename": self.source.name, "sha256": hashlib.sha256(b"sources").hexdigest(), "bytes": 7,
                        "builder_sha256": hashlib.sha256(b"builder").hexdigest(),
                        "sources_lock_sha256": hashlib.sha256(b'{"source_set_id":"nova-test"}\n').hexdigest(), "source_set_id": "nova-test"}
        self.receipts = []
        for index, platform in enumerate(("macos-arm64", "windows-x86_64", "linux-x86_64")):
            extension = ".tar.gz" if platform == "linux-x86_64" else ".zip"
            self.receipts.append({"schema_version": 1, "status": "PASS", "version": "1.2.3", "platform": platform, "commit": self.commit,
                                  "source_bundle": copy.deepcopy(self.binding),
                                  "archive": {"filename": f"Nova-Flux-1.2.3-{platform}{extension}", "sha256": str(index + 1) * 64, "bytes": 100 + index},
                                  "package_manifest_sha256": "b" * 64,
                                  "native_tests": {"executables": 1, "passed": 10 + index, "failed": 0, "ignored": 0}})
        self.output = self.base / "aggregate"

    def tearDown(self):
        self.temporary.cleanup()

    def aggregate(self, receipts=None):
        paths = []
        for index, receipt in enumerate(self.receipts if receipts is None else receipts):
            path = self.base / f"receipt-{index}.json"
            path.write_text(json.dumps(receipt), encoding="utf-8")
            paths.append(path)
        return aggregate_public_release.aggregate(paths, self.source, self.output, expected_commit=self.commit, root=self.root)

    def test_all_native_receipts_bind_actual_source_bytes_and_checksum_list(self):
        report = self.aggregate()
        self.assertEqual(report["status"], "PASS")
        self.assertEqual(report["source_bundle"], self.binding)
        self.assertEqual({p["platform"] for p in report["platforms"]}, {"macos-arm64", "windows-x86_64", "linux-x86_64"})
        lines = (self.output / "SHA256SUMS").read_text().splitlines()
        self.assertEqual(len(lines), 4)
        self.assertIn("1" * 64 + "  Nova-Flux-1.2.3-macos-arm64.zip", lines)
        self.assertIn(hashlib.sha256(b"sources").hexdigest() + "  Nova-Flux-1.2.3-FFmpeg-sources.zip", lines)
        self.assertIn("not downloaded", report["package_byte_verification"])
        self.assertEqual(json.loads((self.output / "report.json").read_text())["commit"], self.commit)

    def test_receipt_source_hash_must_agree_across_platforms(self):
        self.receipts[1]["source_bundle"]["sha256"] = "0" * 64
        with self.assertRaises(ValueError):
            self.aggregate()

    def test_common_zip_bytes_are_verified_independently_of_receipts(self):
        self.source.write_bytes(b"changed")
        with self.assertRaises(ValueError):
            self.aggregate()

    def test_all_three_unique_platforms_are_required(self):
        with self.assertRaises(ValueError):
            self.aggregate(self.receipts[:2])
        self.receipts[2] = copy.deepcopy(self.receipts[0])
        with self.assertRaises(ValueError):
            self.aggregate()

    def test_one_or_all_stale_commits_are_rejected_against_current_run(self):
        self.receipts[0]["commit"] = "f" * 40
        with self.assertRaises(ValueError):
            self.aggregate()
        for receipt in self.receipts:
            receipt["commit"] = "f" * 40
        with self.assertRaises(ValueError):
            self.aggregate()

    def test_version_and_frozen_recipe_binding_must_match_checked_out_source(self):
        for key in ("builder_sha256", "sources_lock_sha256", "source_set_id"):
            with self.subTest(key=key):
                original = self.receipts[0]["source_bundle"][key]
                self.receipts[0]["source_bundle"][key] = "0" * 64
                with self.assertRaises(ValueError):
                    self.aggregate()
                self.receipts[0]["source_bundle"][key] = original
        self.receipts[0]["version"] = "0.0.1"
        with self.assertRaises(ValueError):
            self.aggregate()

    def test_failed_ignored_or_empty_test_runs_cannot_be_aggregated_as_pass(self):
        for key, value in (("failed", 1), ("ignored", 1), ("passed", 0), ("executables", 0)):
            with self.subTest(key=key):
                original = self.receipts[0]["native_tests"][key]
                self.receipts[0]["native_tests"][key] = value
                with self.assertRaises(ValueError):
                    self.aggregate()
                self.receipts[0]["native_tests"][key] = original
        self.receipts[0]["status"] = "FAIL"
        with self.assertRaises(ValueError):
            self.aggregate()

    def test_archive_checksum_names_cannot_escape_or_inject_lines(self):
        self.receipts[0]["archive"]["filename"] = "../outside.zip\nextra"
        with self.assertRaises(ValueError):
            self.aggregate()


if __name__ == "__main__":
    unittest.main()
