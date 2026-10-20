"""Public packaging boundary tests; fixtures are not native release acceptance."""
from __future__ import annotations
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
import zipfile
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
try:
    import package_verified as package
except ModuleNotFoundError:
    package = None


class VerifiedPackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def write(self, name, data):
        file = self.root / name
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_bytes(data)
        return file

    def test_inventory_rejects_tamper_and_unlisted_files(self):
        self.assertIsNotNone(package, "verified packager has not been implemented")
        tree = self.root / "payload"
        self.write("payload/LICENSE", b"Original license")
        records = [{"path": "LICENSE", "bytes": 16, "sha256": hashlib.sha256(b"Original license").hexdigest()}]
        package.verify_records(tree, records)
        self.write("payload/LICENSE", b"Changed license!")
        with self.assertRaisesRegex(ValueError, "integrity"):
            package.verify_records(tree, records)
        self.write("payload/LICENSE", b"Original license")
        self.write("payload/unlisted", b"extra")
        with self.assertRaisesRegex(ValueError, "inventory"):
            package.verify_records(tree, records)

    def test_inventory_rejects_symlink_and_traversal(self):
        self.assertIsNotNone(package, "verified packager has not been implemented")
        self.write("outside", b"private")
        tree = self.root / "payload"
        tree.mkdir()
        (tree / "link").symlink_to(self.root / "outside")
        with self.assertRaisesRegex(ValueError, "symlink"):
            package.inventory(tree)
        with self.assertRaises(ValueError):
            package.verify_records(tree, [{"path": "../outside", "bytes": 7, "sha256": "0" * 64}])

    def test_notice_gate_rejects_wrong_target_before_packaging(self):
        self.assertIsNotNone(package, "verified packager has not been implemented")
        self.write("Cargo.toml", b'[package]\nname="nova-flux"\nversion="2.3.1"\n')
        self.write("Cargo.lock", b'version=4\npackage=[]\n')
        self.write("notices/manifest.json", json.dumps({"schema_version": 1, "target": "x86_64-pc-windows-msvc"}).encode())
        with self.assertRaisesRegex(ValueError, "target"):
            package.verify_notices(self.root / "notices", "macos-arm64", self.root)

    def test_notice_gate_rejects_stale_application_or_lock(self):
        self.assertIsNotNone(package, "verified packager has not been implemented")
        self.write("Cargo.toml", b'[package]\nname="nova-flux"\nversion="2.3.1"\n')
        self.write("Cargo.lock", b'version=4\npackage=[]\n')
        self.write("notices/manifest.json", json.dumps({"schema_version": 1, "target": "aarch64-apple-darwin", "application": {"name": "nova-flux", "version": "2.3.0"}, "cargo_lock_sha256": "0" * 64}).encode())
        with self.assertRaisesRegex(ValueError, "stale application version/Cargo.lock"):
            package.verify_notices(self.root / "notices", "macos-arm64", self.root)

    def test_windows_application_rejects_vc_redistributable_import(self):
        self.assertIsNotNone(package, "verified packager has not been implemented")
        # Native inspection is an external tool boundary; exercise the real
        # DLL parser and allowlist while supplying its captured text output.
        with mock.patch.object(package, "windows_objdump", return_value="objdump"), mock.patch.object(package.subprocess, "check_output", return_value="    DLL Name: KERNEL32.dll\n    DLL Name: VCRUNTIME140.dll\n"):
            with self.assertRaisesRegex(ValueError, "unapproved"):
                package.application_imports(self.root / "nova-flux.exe", "windows-x86_64")
        with mock.patch.object(package, "windows_objdump", return_value="objdump"), mock.patch.object(package.subprocess, "check_output", return_value="    DLL Name: KERNEL32.dll\n    DLL Name: USER32.dll\n"):
            self.assertEqual(package.application_imports(self.root / "nova-flux.exe", "windows-x86_64"), ["KERNEL32.dll", "USER32.dll"])

    def test_windows_inspection_finds_msys2_in_runner_temporary_directory(self):
        tool = self.write("msys64/ucrt64/bin/objdump.exe", b"inspection fixture")
        with mock.patch.dict(os.environ, {"RUNNER_TEMP": str(self.root)}, clear=True), mock.patch.object(package.shutil, "which", return_value=None):
            self.assertEqual(package.windows_objdump(), str(tool.resolve()))

    def test_archived_package_roundtrip_detects_changed_extracted_file(self):
        self.assertIsNotNone(package, "verified packager has not been implemented")
        stage = self.root / "Nova"
        self.write("Nova/application", b"fixture executable")
        (stage / "application").chmod(0o755)
        record = {"path": "application", "bytes": 18, "sha256": hashlib.sha256(b"fixture executable").hexdigest()}
        self.write("Nova/package-manifest.json", json.dumps({"schema_version": 1, "files": [record]}).encode())
        for suffix in (".zip", ".tar.gz"):
            archive = self.root / ("release" + suffix)
            package.write_archive(stage, archive)
            extracted = package.extract_checked(archive, self.root / ("extracted" + suffix), "Nova")
            package.verify_package_tree(extracted)
            if os.name != "nt":
                self.assertTrue((extracted / "application").stat().st_mode & 0o111)
            (extracted / "application").write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "integrity"):
                package.verify_package_tree(extracted)

    def test_source_binding_rejects_stale_builder_and_lock(self):
        self.assertIsNotNone(package, "verified packager has not been implemented")
        expected = {"source_set_id": "set", "sources_lock_sha256": "a" * 64, "builder_sha256": "b" * 64, "source_archives": []}
        package.verify_source_binding(dict(expected), expected)
        for key in ("sources_lock_sha256", "builder_sha256", "source_set_id"):
            changed = dict(expected, **{key: "stale"})
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "source"):
                package.verify_source_binding(changed, expected)

    def test_archive_extraction_preserves_bytes_and_rejects_escape(self):
        self.assertIsNotNone(package, "verified packager has not been implemented")
        archive = self.root / "package.zip"
        with zipfile.ZipFile(archive, "w") as output:
            output.writestr("Nova/LICENSE", b"exact license\r\n")
        extracted = self.root / "extract"
        package.extract_checked(archive, extracted, "Nova")
        self.assertEqual((extracted / "Nova/LICENSE").read_bytes(), b"exact license\r\n")
        with zipfile.ZipFile(self.root / "bad.zip", "w") as output:
            output.writestr("Nova/../../outside", b"escape")
        with self.assertRaises(ValueError):
            package.extract_checked(self.root / "bad.zip", self.root / "bad-extract", "Nova")

    def test_privacy_gate_rejects_actual_home_but_keeps_upstream_authorship(self):
        self.assertIsNotNone(package, "verified packager has not been implemented")
        tree = self.root / "payload"
        self.write("payload/LICENSE", b"Copyright Steven Sheldon and Nova authors")
        package.verify_privacy(tree, private_tokens=[b"/Users/private-user"])
        self.write("payload/application", b"code\0/Users/private-user/project/source.rs\0")
        with self.assertRaisesRegex(ValueError, "private local path"):
            package.verify_privacy(tree, private_tokens=[b"/Users/private-user"])

    def test_generated_windows_log_normalization_preserves_inputs_and_records_change(self):
        runtime = self.root / "runtime"
        original = b"USERPROFILE='C:\\Users\\runneradmin'\nCFLAGS=-O2\n"
        self.write("runtime/provenance/configuration/ffmpeg/ffbuild/config.log", original)
        binary = self.write("runtime/bin/ffmpeg.exe", b"binary bytes")
        notice = self.write("runtime/licenses/LICENSE", b"Original author C:\\Users\\runneradmin")
        value = {"schema_version": 1, "files": package.inventory(runtime)}
        self.write("runtime/manifest.json", json.dumps(value).encode())
        changes = package.normalize_generated_provenance(runtime, [(r"C:\Users\runneradmin", "{HOME}")])
        self.assertEqual(len(changes), 1)
        self.assertEqual(changes[0]["original_sha256"], hashlib.sha256(original).hexdigest())
        self.assertEqual(binary.read_bytes(), b"binary bytes")
        self.assertEqual(notice.read_bytes(), b"Original author C:\\Users\\runneradmin")
        updated = json.loads((runtime / "manifest.json").read_text())
        package.verify_records(runtime, updated["files"], exclude=("manifest.json",))
        self.assertEqual((runtime / changes[0]["path"]).read_bytes(), b"USERPROFILE='{HOME}'\nCFLAGS=-O2\n")

    def test_package_notice_uses_actual_mac_bundle_paths(self):
        text = package.package_notice("2.3.1", "macos-arm64", "Nova Flux.app/Contents/Resources/licenses", "Nova Flux.app/Contents/Resources/runtime", "https://example.org/source.zip", "a" * 64)
        self.assertIn("Nova Flux.app/Contents/Resources/licenses/application-LICENSE.txt", text)
        self.assertIn("Nova Flux.app/Contents/Resources/runtime/source/", text)
        self.assertIn("relative to the extracted top-level", text)


if __name__ == "__main__":
    unittest.main()
