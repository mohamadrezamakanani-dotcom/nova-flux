"""Release-gate tests for exact crate notices, not live registry/network tests."""
from __future__ import annotations

import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

try:
    import collect_binary_notices as notices
except ModuleNotFoundError:
    notices = None


class BinaryNoticesTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def crate(self, name="example", version="1.2.3", license="MIT", files=None):
        base = self.root / "registry/src/index" / f"{name}-{version}"
        base.mkdir(parents=True)
        archive = self.root / "registry/cache/index" / f"{name}-{version}.crate"
        archive.parent.mkdir(parents=True, exist_ok=True)
        contents = {"Cargo.toml": f'[package]\nname="{name}"\nversion="{version}"\nlicense="{license}"\n'.encode()}
        contents.update(files or {})
        with tarfile.open(archive, "w:gz") as output:
            for file, data in contents.items():
                item = tarfile.TarInfo(f"{name}-{version}/{file}")
                item.size = len(data)
                output.addfile(item, io.BytesIO(data))
        checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
        package = {"id": f"registry+https://github.com/rust-lang/crates.io-index#{name}@{version}", "name": name, "version": version, "license": license, "license_file": None, "repository": None, "source": "registry+https://github.com/rust-lang/crates.io-index", "manifest_path": str(base / "Cargo.toml")}
        return package, checksum, archive

    def test_preserves_exact_notice_bytes_and_records_archive_checksum(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        package, checksum, _ = self.crate(files={"LICENSE": b"Copyright Example\r\nPermission is hereby granted.\r\n", "nested/NOTICE": b"Original NOTICE\n"})
        result, files = notices.collect_crate(package, checksum)
        self.assertEqual(result["checksum_sha256"], checksum)
        self.assertEqual(files["LICENSE"], b"Copyright Example\r\nPermission is hereby granted.\r\n")
        self.assertEqual(files["nested/NOTICE"], b"Original NOTICE\n")
        self.assertEqual(result["license_expression"], "MIT")

    def test_rejects_tampered_archive_before_reading_notices(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        package, checksum, archive = self.crate(files={"LICENSE": b"Permission is hereby granted."})
        archive.write_bytes(archive.read_bytes() + b"tampered")
        with self.assertRaisesRegex(ValueError, "checksum"):
            notices.collect_crate(package, checksum)

    def test_rejects_unknown_license_even_if_license_file_exists(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        package, checksum, _ = self.crate(license="LicenseRef-Unknown", files={"LICENSE": b"Unreviewed text"})
        with self.assertRaisesRegex(ValueError, "Unreviewed license"):
            notices.collect_crate(package, checksum)

    def test_fails_closed_when_required_license_text_is_missing(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        package, checksum, _ = self.crate()
        with self.assertRaisesRegex(ValueError, "Missing license"):
            notices.collect_crate(package, checksum)

    def test_font_notices_include_hack_and_fail_if_ubuntu_text_is_missing(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        files = {"LICENSE-MIT": b"Copyright egui\nPermission is hereby granted.", "fonts/Hack-Regular.txt": b"Copyright Hack\nBitstream Vera terms", "fonts/OFL.txt": b"SIL OPEN FONT LICENSE", "fonts/UFL.txt": b"UBUNTU FONT LICENCE", "fonts/emoji-icon-font-mit-license.txt": b"Copyright emoji\nMIT text"}
        expression = "(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0"
        package, checksum, _ = self.crate(name="epaint_default_fonts", version="0.36.2", license=expression, files=files)
        _, found = notices.collect_crate(package, checksum)
        self.assertEqual(found["fonts/Hack-Regular.txt"], b"Copyright Hack\nBitstream Vera terms")
        del files["fonts/UFL.txt"]
        package, checksum, _ = self.crate(name="epaint_default_fonts", version="0.36.3", license=expression, files=files)
        with self.assertRaisesRegex(ValueError, "fonts/UFL.txt"):
            notices.collect_crate(package, checksum)

    def test_target_selection_excludes_development_and_unselected_crates(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        package, _, _ = self.crate()
        unused = dict(package, id="unused", name="unused")
        selected = notices.select_packages({"packages": [package, unused]}, "nova-flux v2.3.0 (/workspace)\nexample v1.2.3\nexample v1.2.3 (*)\n", "nova-flux")
        self.assertEqual([item["id"] for item in selected], [package["id"]])

    def test_retains_inline_copyright_and_license_header_verbatim(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        header = b"// Copyright 2001 Original Author\n// Permission is hereby granted to copy this source.\n\n"
        package, checksum, _ = self.crate(files={"LICENSE": b"Permission is hereby granted.", "src/lib.rs": header + b"pub fn code() {}\n"})
        _, found = notices.collect_crate(package, checksum)
        self.assertEqual(found["inline/src/lib.rs.txt"], header)

    def test_only_exact_reviewed_dispatch_archive_gets_canonical_supplement(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        contents = {"Cargo.toml": b'[package]\nname="dispatch"\nversion="0.2.0"\nlicense="MIT"\nauthors=["Steven Sheldon"]\n', "README.md": b"Original dispatch README", "src/lib.rs": b"// Original source\n", ".cargo_vcs_info.json": b'{"git":{"sha1":"82d6c7a5b75dc0c71c3f46f87bb6c16a476f7748"}}'}
        files, record = notices.dispatch_supplement("dispatch", "0.2.0", "bd0c93bb4b0c6d9b77f4435b0ae98c24d17f1c45b2ff844c6151a07256ca923b", contents)
        self.assertEqual(files["original-source/Cargo.toml"], contents["Cargo.toml"])
        self.assertEqual(files["original-source/src/lib.rs"], b"// Original source\n")
        self.assertIn(b"Permission is hereby granted", files["supplement/MIT-permission.txt"])
        self.assertNotIn(b"Copyright", files["supplement/MIT-permission.txt"])
        self.assertEqual(record["status"], "reviewed-upstream-notice-omission")
        with self.assertRaisesRegex(ValueError, "Missing license"):
            notices.dispatch_supplement("dispatch", "0.2.1", "0" * 64, contents)

    def test_toolchain_notices_retain_standard_library_and_complete_copyright(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        doc = self.root / "sysroot/share/doc/rust"
        (doc / "licenses").mkdir(parents=True)
        (doc / "COPYRIGHT-library.html").write_bytes(b"Original standard-library notices")
        (doc / "COPYRIGHT.html").write_bytes(b"Original complete toolchain notices")
        (doc / "licenses/MIT.txt").write_bytes(b"Original MIT text")
        (doc / "licenses/Apache-2.0.txt").write_bytes(b"Original Apache text")
        record, files = notices.toolchain_notices(self.root / "sysroot", "rustc 1.99.0\ncommit-hash: abcdef\nhost: aarch64-apple-darwin\n")
        self.assertEqual(files["COPYRIGHT-library.html"], b"Original standard-library notices")
        self.assertEqual(files["COPYRIGHT.html"], b"Original complete toolchain notices")
        self.assertEqual(record["rustc_version_verbose"], "rustc 1.99.0\ncommit-hash: abcdef\nhost: aarch64-apple-darwin\n")
        (doc / "COPYRIGHT-library.html").unlink()
        with self.assertRaisesRegex(ValueError, "toolchain notice"):
            notices.toolchain_notices(self.root / "sysroot", "rustc 1.99.0")

    def test_xml_registry_copyright_comment_is_preserved(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        header = b'<?xml version="1.0"?>\n<registry>\n    <comment>\nCopyright Khronos\nMIT permission\n    </comment>\n'
        self.assertEqual(notices.inline_header(header + b'    <types/>\n</registry>'), header)

    def test_rejects_unreviewed_mit_declaration_without_permission_text(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        package, checksum, _ = self.crate(files={"LICENSE": b"Licensed under MIT; see external URL"})
        with self.assertRaisesRegex(ValueError, "Missing selected MIT permission"):
            notices.collect_crate(package, checksum)

    def test_rejects_modified_retained_upstream_tree_cache(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        cache = self.root / "cache"
        cache.mkdir()
        url = "https://api.github.com/repos/example/project/git/trees/" + "0" * 40 + "?recursive=1"
        key = hashlib.sha256(url.encode()).hexdigest()
        data = b'{"tree":[]}'
        (cache / key).write_bytes(data + b"tampered")
        (cache / "cache-manifest.json").write_text(json.dumps({"schema_version": 1, "files": [{"path": key, "url": url, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}]}))
        with self.assertRaisesRegex(ValueError, "Retained upstream cache"):
            notices.UpstreamNotices(cache).read(url)

    def test_public_cargo_tree_keeps_versions_but_removes_local_source_root(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        original = "nova-flux v2.3.1 (/Users/private-user/work/nova)\nserde v1.0.0\n"
        self.assertEqual(notices.public_tree(original, Path("/Users/private-user/work/nova")), "nova-flux v2.3.1 ({SOURCE})\nserde v1.0.0\n")

    def test_rejects_upstream_git_blob_mismatch(self):
        self.assertIsNotNone(notices, "notice collector has not been implemented")
        with self.assertRaisesRegex(ValueError, "Git blob"):
            notices.verify_git_blob(b"changed", "0" * 40)


if __name__ == "__main__":
    unittest.main()
