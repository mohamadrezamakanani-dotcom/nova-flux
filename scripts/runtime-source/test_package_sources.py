"""Source closure, exact-byte provenance, deterministic ZIP, and tamper tests."""

import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile


SPEC = importlib.util.spec_from_file_location("package_sources", Path(__file__).with_name("package_sources.py"))
package = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(package)


class SourceBundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.repo = self.root / "repository"
        self.recipes = self.repo / "scripts/runtime-source"
        self.recipes.mkdir(parents=True)
        self.cache = self.root / "cache"
        (self.cache / "archives").mkdir(parents=True)
        (self.repo / "LICENSE").write_bytes(b"MIT permission for Nova code.\n")
        for name in package.RECIPE_FILES + package.SUPPORT_FILES:
            (self.recipes / name).write_bytes((f"Exact recipe bytes: {name}\n").encode())
        sources = []
        for name in sorted(package.SOURCE_NAMES):
            license_path = "COPYING.GPLv3" if name == "ffmpeg" else "COPYING"
            license_data = (f"Original {name} license notice.\n").encode()
            archive_path = self.cache / "archives" / f"{name}-1.tar.gz"
            with tarfile.open(archive_path, "w:gz") as archive:
                for relative, data in ((license_path, license_data), ("source.c", b"int answer(void) {return 42;}\n")):
                    member = tarfile.TarInfo(f"{name}-1/{relative}")
                    member.size = len(data)
                    archive.addfile(member, io.BytesIO(data))
            sources.append({
                "name": name, "version": "1", "archive": archive_path.name,
                "url": f"https://upstream.invalid/{archive_path.name}", "archive_root": f"{name}-1", "source_dir": f"{name}-1",
                "archive_bytes": archive_path.stat().st_size, "sha256": package.acquire.sha256_file(archive_path), "patches": [],
                "license": {"expression": "fixture", "files": [license_path], "file_hashes": [{"path": license_path, "sha256": hashlib.sha256(license_data).hexdigest(), "bytes": len(license_data)}]},
            })
        self.lock = {"schema_version": 1, "source_set_id": "nova-test", "patches": [], "sources": sources}
        self.lock_path = self.recipes / "sources.lock.json"
        self.lock_path.write_bytes(package.json_bytes(self.lock))
        self.destination = self.root / "staging"
        self.output = self.root / "sources.zip"

    def create(self, **kwargs):
        return package.package_sources(self.cache, self.destination, self.output, recipe_dir=self.recipes, repository=self.repo, **kwargs)

    def payload(self):
        with zipfile.ZipFile(self.output) as archive:
            files = {info.filename.split("/", 1)[1]: archive.read(info) for info in archive.infolist()}
        return files, json.loads(files[package.MANIFEST_NAME])

    def rewrite(self, files, manifest=None):
        if manifest is not None:
            files[package.MANIFEST_NAME] = package.json_bytes(manifest)
        package.write_zip(self.output, "nova-test-sources", files)

    def test_complete_original_archives_exact_recipes_and_gpl_are_retained(self):
        (self.cache / "archives/ffmpeg-9.0.2-unused.tar.xz").write_bytes(b"unused research archive")
        result = self.create()
        manifest = package.verify_zip(self.output, result["sha256"])
        files, _ = self.payload()
        self.assertEqual(len(manifest["source_archives"]), 9)
        self.assertFalse(any("unused" in path for path in files))
        for source in self.lock["sources"]:
            self.assertEqual(files["archives/" + source["archive"]], (self.cache / "archives" / source["archive"]).read_bytes())
        for name in package.RECIPE_FILES:
            self.assertEqual(files[package.PREFIX + name], (self.recipes / name).read_bytes())
        self.assertEqual(files["COPYING.GPLv3"], files["licenses/ffmpeg/COPYING.GPLv3"])
        self.assertEqual(manifest["included_platform_builds"], [])

    def test_repeat_packaging_is_deterministic_despite_input_mtime_changes(self):
        first = self.create()
        for path in self.cache.rglob("*"):
            if path.is_file():
                os.utime(path, (1_000_000_000, 1_000_000_000))
        second_output = self.root / "sources-second.zip"
        second = package.package_sources(self.cache, self.destination, second_output, recipe_dir=self.recipes, repository=self.repo)
        self.assertEqual(first["sha256"], second["sha256"])
        self.assertEqual(self.output.read_bytes(), second_output.read_bytes())

    def test_changed_or_missing_original_archive_fails(self):
        archive = self.cache / "archives" / self.lock["sources"][0]["archive"]
        archive.write_bytes(b"changed source")
        with self.assertRaisesRegex(package.SourceError, "original bytes"):
            self.create()
        archive.unlink()
        with self.assertRaisesRegex(package.SourceError, "Missing regular"):
            self.create()
        self.assertFalse(self.output.exists())

    def test_missing_recipe_fails(self):
        (self.recipes / "build_runtime.py").unlink()
        with self.assertRaisesRegex(package.SourceError, "Missing regular"):
            self.create()

    def test_changed_payload_zip_fails_internal_verification(self):
        self.create()
        files, _ = self.payload()
        files[package.PREFIX + "build_runtime.py"] += b"changed recipe"
        self.rewrite(files)
        with self.assertRaisesRegex(package.SourceError, "integrity mismatch"):
            package.verify_zip(self.output)

    def test_missing_archive_cannot_be_hidden_by_trimming_file_inventory(self):
        self.create()
        files, manifest = self.payload()
        key = "archives/" + self.lock["sources"][0]["archive"]
        del files[key]
        manifest["files"] = [entry for entry in manifest["files"] if entry["path"] != key]
        self.rewrite(files, manifest)
        with self.assertRaisesRegex(package.SourceError, "exactly the lock-selected"):
            package.verify_zip(self.output)

    def test_unlisted_file_and_unsafe_zip_path_fail(self):
        self.create()
        files, _ = self.payload()
        files["unexpected.txt"] = b"extra"
        self.rewrite(files)
        with self.assertRaisesRegex(package.SourceError, "unlisted"):
            package.verify_zip(self.output)
        with zipfile.ZipFile(self.output, "a") as archive:
            archive.writestr("nova-test-sources/../../escape", b"escape")
        with self.assertRaisesRegex(package.SourceError, "Unsafe source path"):
            package.verify_zip(self.output)

    def test_platform_manifest_must_match_bundled_source_lock_and_builder(self):
        path = self.root / "native-build.json"
        value = {"platform": "macos-arm64", "source_set_id": "nova-test", "sources_lock_sha256": package.acquire.sha256_file(self.lock_path), "builder_sha256": "0" * 64, "patches": []}
        path.write_bytes(package.json_bytes(value))
        with self.assertRaisesRegex(package.SourceError, "exact bundled"):
            self.create(build_manifests=[path])
        value["builder_sha256"] = package.acquire.sha256_file(self.recipes / "build_runtime.py")
        path.write_bytes(package.json_bytes(value))
        self.create(build_manifests=[path])
        self.assertEqual(package.verify_zip(self.output)["included_platform_builds"], ["macos-arm64"])

    def test_altered_existing_stage_and_output_are_preserved(self):
        result = self.create()
        stage_file = Path(result["staging_directory"]) / "INTRODUCTION.md"
        stage_file.write_bytes(b"local review edit")
        with self.assertRaisesRegex(package.SourceError, "preserved"):
            self.create()
        self.assertEqual(stage_file.read_bytes(), b"local review edit")
        self.assertEqual(package.acquire.sha256_file(self.output), result["sha256"])
        stage_file.write_bytes(self.payload()[0]["INTRODUCTION.md"])
        self.output.write_bytes(b"existing unrelated ZIP")
        with self.assertRaisesRegex(package.SourceError, "Existing output ZIP differs"):
            self.create()
        self.assertEqual(self.output.read_bytes(), b"existing unrelated ZIP")

    def test_expected_published_digest_mismatch_fails(self):
        self.create()
        with self.assertRaisesRegex(package.SourceError, "published value"):
            package.verify_zip(self.output, "0" * 64)


if __name__ == "__main__":
    unittest.main()
