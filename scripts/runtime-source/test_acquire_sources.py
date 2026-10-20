"""Integrity, extraction boundary, and offline source-acquisition checks."""

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location("acquire_sources", Path(__file__).with_name("acquire_sources.py"))
acquire = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(acquire)


class SourceAcquisitionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.cache = self.root / "cache"
        self.archive_dir = self.cache / "archives"
        self.archive_dir.mkdir(parents=True)
        self.archive_path = self.archive_dir / "example-1.tar.gz"
        self.license_data = b"Retain this copyright and permission notice.\n"
        self.write_archive()

    def write_archive(self, extra=None):
        with tarfile.open(self.archive_path, "w:gz") as archive:
            for name, data, mode in (("example-1/COPYING", self.license_data, 0o644), ("example-1/configure", b"#!/bin/sh\nexit 0\n", 0o755)):
                member = tarfile.TarInfo(name)
                member.size, member.mode = len(data), mode
                archive.addfile(member, io.BytesIO(data))
            if extra:
                member, data = extra
                archive.addfile(member, io.BytesIO(data) if data is not None else None)
        self.source = {
            "name": "example", "version": "1", "url": "https://upstream.invalid/example-1.tar.gz",
            "archive": self.archive_path.name, "archive_root": "example-1", "source_dir": "example-1",
            "sha256": acquire.sha256_file(self.archive_path), "archive_bytes": self.archive_path.stat().st_size,
            "license": {"files": ["COPYING"], "file_hashes": [{"path": "COPYING", "sha256": hashlib.sha256(self.license_data).hexdigest(), "bytes": len(self.license_data)}]},
            "patches": [],
        }
        self.lock = self.root / "lock.json"
        self.lock.write_text(json.dumps({"schema_version": 1, "source_set_id": "test", "sources": [self.source]}))

    def run_acquisition(self, **kwargs):
        return acquire.acquire_sources(self.lock, self.cache, offline=True, **kwargs)

    def test_retains_original_archive_license_and_executable_mode(self):
        original = self.archive_path.read_bytes()
        manifest = self.run_acquisition(extract=True)
        self.assertEqual(original, self.archive_path.read_bytes())
        self.assertEqual(self.license_data, (self.cache / "licenses/example/COPYING").read_bytes())
        source_dir = self.cache / "extracted/example-1"
        if os.name != "nt":
            self.assertTrue((source_dir / "configure").stat().st_mode & 0o111)
        self.assertEqual(manifest["sources"][0]["source_tree_sha256"], acquire.digest_records(acquire.tree_records(source_dir)))
        self.assertEqual(manifest, self.run_acquisition(extract=True))

    def test_changed_archive_is_rejected_and_preserved(self):
        self.archive_path.write_bytes(b"changed")
        with self.assertRaisesRegex(acquire.SourceError, "integrity mismatch"):
            self.run_acquisition(extract=True)
        self.assertEqual(b"changed", self.archive_path.read_bytes())
        self.assertFalse((self.cache / "extracted").exists())

    def test_changed_extracted_tree_is_rejected_and_preserved(self):
        self.run_acquisition(extract=True)
        source = self.cache / "extracted/example-1/configure"
        source.write_bytes(b"local patch")
        with self.assertRaisesRegex(acquire.SourceError, "Existing source tree differs"):
            self.run_acquisition(extract=True)
        self.assertEqual(source.read_bytes(), b"local patch")

    def test_missing_offline_archive_does_not_open_network(self):
        self.archive_path.unlink()
        with mock.patch.object(acquire.urllib.request, "build_opener") as opener:
            with self.assertRaisesRegex(acquire.SourceError, "offline"):
                self.run_acquisition()
            opener.assert_not_called()

    def test_verify_only_writes_nothing(self):
        before = set(self.cache.rglob("*"))
        with mock.patch.object(acquire.urllib.request, "build_opener") as opener:
            self.run_acquisition(verify_only=True, extract=True)
            opener.assert_not_called()
        self.assertEqual(before, set(self.cache.rglob("*")))

    def test_unsafe_tar_paths_are_rejected_before_extraction(self):
        for path in ("../escape", "/absolute", "example-1/../../escape", "example-1/back\\slash", "C:/drive", "example-1/C:/drive", "example-1/file:stream", "example-1/file.", "other-root/file"):
            with self.subTest(path=path):
                member = tarfile.TarInfo(path)
                member.size = 1
                self.write_archive((member, b"x"))
                with self.assertRaises(acquire.SourceError):
                    self.run_acquisition(extract=True)
                self.assertFalse((self.cache / "extracted").exists())

    def test_links_devices_and_duplicate_paths_are_rejected(self):
        for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE, tarfile.CHRTYPE):
            with self.subTest(kind=kind):
                member = tarfile.TarInfo("example-1/unsafe")
                member.type, member.linkname = kind, "../../escape"
                self.write_archive((member, None))
                with self.assertRaisesRegex(acquire.SourceError, "links and special"):
                    self.run_acquisition(extract=True)
        duplicate = tarfile.TarInfo("example-1/COPYING")
        duplicate.size = 1
        self.write_archive((duplicate, b"x"))
        with self.assertRaisesRegex(acquire.SourceError, "duplicate"):
            self.run_acquisition(extract=True)

    def test_pinned_license_must_match_archive(self):
        self.source["license"]["file_hashes"][0]["sha256"] = "0" * 64
        self.lock.write_text(json.dumps({"schema_version": 1, "source_set_id": "test", "sources": [self.source]}))
        with self.assertRaisesRegex(acquire.SourceError, "License integrity"):
            self.run_acquisition()

    def test_cache_symlink_cannot_redirect_license_write(self):
        outside = self.root / "outside"
        outside.mkdir()
        (self.cache / "licenses").symlink_to(outside, target_is_directory=True)
        with self.assertRaisesRegex(acquire.SourceError, "symlink"):
            self.run_acquisition()
        self.assertEqual([], list(outside.iterdir()))

    def test_download_hash_mismatch_leaves_no_archive_or_partial(self):
        self.archive_path.unlink()
        response = io.BytesIO(b"x" * self.source["archive_bytes"])
        response.geturl = lambda: self.source["url"]
        opener = mock.Mock()
        opener.open.return_value = response
        with mock.patch.object(acquire.urllib.request, "build_opener", return_value=opener):
            with self.assertRaisesRegex(acquire.SourceError, "integrity mismatch"):
                acquire.acquire_sources(self.lock, self.cache)
        self.assertEqual([], list(self.archive_dir.iterdir()))


if __name__ == "__main__":
    unittest.main()
