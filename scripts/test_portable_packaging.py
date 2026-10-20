#!/usr/bin/env python3
"""Boundary tests for build-time portable packaging, without foreign binaries."""
from __future__ import annotations

import io
import json
import os
from pathlib import Path
import struct
import sys
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parent))

from fetch_portable_runtime import download
from package_portable import package
from portable_common import binary_architecture, checked_member, files_manifest, inspect_runtime, safe_extract, verify_files


class PortableBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self):
        self.temporary.cleanup()

    def test_rejects_paths_which_escape_alias_or_break_on_windows(self):
        for name in ("../outside", "/absolute", "C:/outside", "a\\outside", "a/./b", "a//b", "a/../b", "a/NUL.txt", "a/file.", "a/file ", "a/stream:ads"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                checked_member(name, set())
        seen = set()
        checked_member("root/LICENSE.txt", seen)
        with self.assertRaises(ValueError):
            checked_member("root/license.TXT", seen)

    def test_zip_extracts_regular_files_and_preserves_bytes(self):
        archive = self.root / "ok.zip"
        with zipfile.ZipFile(archive, "w") as output:
            output.writestr("root/", b"")
            output.writestr("root/LICENSE.txt", b"license\n")
            output.writestr("root/bin/ffmpeg.exe", b"test binary")
        safe_extract(archive, self.root / "extract", "zip")
        self.assertEqual((self.root / "extract/root/bin/ffmpeg.exe").read_bytes(), b"test binary")

    def test_zip_rejects_unix_symlink_and_traversal(self):
        for name, mode in (("root/link", 0o120777), ("../outside", 0o100644)):
            archive = self.root / "bad.zip"
            with zipfile.ZipFile(archive, "w") as output:
                info = zipfile.ZipInfo(name)
                info.create_system = 3
                info.external_attr = mode << 16
                output.writestr(info, b"/outside")
            with self.assertRaises(ValueError):
                safe_extract(archive, self.root / ("extract-" + str(mode)), "zip")
        self.assertFalse((self.root / "outside").exists())

    def test_tar_preserves_executable_mode_and_rejects_hard_links(self):
        archive = self.root / "ok.tar.xz"
        with tarfile.open(archive, "w:xz") as output:
            info = tarfile.TarInfo("root/bin/ffmpeg")
            info.size, info.mode = 4, 0o755
            output.addfile(info, io.BytesIO(b"ELF!"))
        safe_extract(archive, self.root / "extract", "tar.xz")
        result = self.root / "extract/root/bin/ffmpeg"
        self.assertEqual(result.read_bytes(), b"ELF!")
        if os.name != "nt":
            self.assertEqual(result.stat().st_mode & 0o777, 0o755)
        archive = self.root / "link.tar.xz"
        with tarfile.open(archive, "w:xz") as output:
            info = tarfile.TarInfo("root/alias")
            info.type, info.linkname = tarfile.LNKTYPE, "/outside"
            output.addfile(info)
        with self.assertRaises(ValueError):
            safe_extract(archive, self.root / "link-extract", "tar.xz")

    def test_executable_architecture_checks_machine_and_word_size(self):
        elf = bytearray(64)
        elf[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", elf, 18, 62)
        path = self.root / "tool"
        path.write_bytes(elf)
        self.assertEqual(binary_architecture(path), "elf-x86_64")
        elf[4] = 1
        path.write_bytes(elf)
        with self.assertRaises(ValueError):
            binary_architecture(path)
        pe = bytearray(154)
        pe[:2] = b"MZ"
        struct.pack_into("<I", pe, 60, 128)
        pe[128:132] = b"PE\0\0"
        struct.pack_into("<H", pe, 132, 0x8664)
        struct.pack_into("<H", pe, 152, 0x20B)
        path.write_bytes(pe)
        self.assertEqual(binary_architecture(path), "pe-x86_64")
        struct.pack_into("<H", pe, 132, 0xAA64)
        path.write_bytes(pe)
        with self.assertRaises(ValueError):
            binary_architecture(path)

    def test_manifest_detects_byte_changes_and_unlisted_files(self):
        (self.root / "license").write_bytes(b"original")
        manifest = files_manifest(self.root)
        verify_files(self.root, manifest)
        (self.root / "license").write_bytes(b"modified")
        with self.assertRaises(ValueError):
            verify_files(self.root, manifest)
        (self.root / "license").write_bytes(b"original")
        (self.root / "extra").write_bytes(b"untracked")
        with self.assertRaises(ValueError):
            verify_files(self.root, manifest)

    def test_corrupt_cached_download_fails_instead_of_using_or_overwriting_it(self):
        archive = self.root / "pinned.zip"
        archive.write_bytes(b"bad")
        with self.assertRaises(ValueError):
            download({"archive": "pinned.zip", "bytes": 3, "sha256": "0" * 64}, self.root)
        self.assertEqual(archive.read_bytes(), b"bad")

    def test_public_package_is_blocked_before_native_execution(self):
        with self.assertRaisesRegex(ValueError, "Public binary packaging is blocked"):
            package("linux-x86_64", self.root, self.root / "app", self.root, "public")

    def test_runtime_requires_cpu_encoders_and_rejects_nonfree(self):
        from portable_common import REQUIRED_ENCODERS
        def fake_output(command, **kwargs):
            if "-encoders" in command:
                return "\n".join(" V..... " + encoder + " description" for encoder in REQUIRED_ENCODERS)
            return "ffmpeg version pinned\nconfiguration: --enable-gpl\n"
        with mock.patch("portable_common.require_host"), mock.patch("portable_common.binary_architecture", return_value="elf-x86_64"), mock.patch("portable_common.run", side_effect=fake_output):
            self.assertEqual(inspect_runtime(self.root, "linux-x86_64", "pinned")["native_runtime_check"], "PASS")
        with mock.patch("portable_common.require_host"), mock.patch("portable_common.binary_architecture", return_value="elf-x86_64"), mock.patch("portable_common.run", return_value="ffmpeg version pinned\n--enable-gpl --enable-nonfree"):
            with self.assertRaisesRegex(ValueError, "nonfree is rejected"):
                inspect_runtime(self.root, "linux-x86_64", "pinned")
        with mock.patch("portable_common.require_host"), mock.patch("portable_common.binary_architecture", return_value="elf-x86_64"), mock.patch("portable_common.run", side_effect=lambda cmd: "ffmpeg version pinned\n--enable-gpl" if "-version" in cmd else " V..... libx264 encoder"):
            with self.assertRaisesRegex(ValueError, "Required CPU encoders missing"):
                inspect_runtime(self.root, "linux-x86_64", "pinned")

    def test_lock_has_exact_assets_and_never_floats_to_latest(self):
        lock = json.loads(Path(__file__).with_name("portable_runtime.lock.json").read_text())
        self.assertEqual(lock["distribution_status"], "private-ci-only")
        for asset in lock["platforms"].values():
            self.assertNotIn("/latest/", asset["url"])
            self.assertRegex(asset["sha256"], r"^[a-f0-9]{64}$")
            self.assertGreater(asset["bytes"], 1000000)


if __name__ == "__main__":
    unittest.main()
