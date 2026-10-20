#!/usr/bin/env python3
"""Package a native Nova release only from verified source/runtime/notices inputs."""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import plistlib
import platform as native_platform
import re
import shutil
import stat
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.parse
import zipfile

ROOT = Path(__file__).resolve().parent.parent
RECIPE = ROOT / "scripts/runtime-source"
TARGETS = {"macos-arm64": "aarch64-apple-darwin", "windows-x86_64": "x86_64-pc-windows-msvc", "linux-x86_64": "x86_64-unknown-linux-gnu"}
SOURCE_RELEASE = "https://github.com/mohamadrezamakanani-dotcom/nova-flux/releases/download"
MANIFEST = "package-manifest.json"


def load_helper(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


builder = load_helper("nova_verified_builder", RECIPE / "build_runtime.py")
source_bundle = load_helper("nova_verified_sources", RECIPE / "package_sources.py")


def digest(path):
    return builder.sha256(Path(path))


def safe_path(name):
    parts = name.split("/")
    if not name or name.startswith("/") or "\\" in name or ":" in name or any(p in {"", ".", ".."} for p in parts):
        raise ValueError(f"Unsafe package path: {name}")
    if any(p.endswith((" ", ".")) or p.split(".")[0].upper() in {"CON", "PRN", "AUX", "NUL", *("COM" + str(i) for i in range(1, 10)), *("LPT" + str(i) for i in range(1, 10))} for p in parts):
        raise ValueError(f"Nonportable package path: {name}")
    return PurePosixPath(name)


def inventory(tree, exclude=()):
    result = []
    if tree.is_symlink() or not tree.is_dir():
        raise ValueError("Package inventory root must be a regular directory")
    seen = set()
    for path in sorted(tree.rglob("*")):
        relative = path.relative_to(tree).as_posix()
        safe_path(relative)
        if relative.casefold() in seen:
            raise ValueError(f"Case-colliding package inventory path: {relative}")
        seen.add(relative.casefold())
        if path.is_symlink():
            raise ValueError(f"Unexpected symlink: {relative}")
        if path.is_file() and relative not in exclude:
            result.append({"path": relative, "bytes": path.stat().st_size, "sha256": digest(path)})
        elif not path.is_file() and not path.is_dir():
            raise ValueError(f"Unexpected special file: {relative}")
    return result


def verify_records(tree, records, exclude=()):
    expected = {}
    for item in records:
        name = item["path"]
        safe_path(name)
        if name in expected or name in exclude:
            raise ValueError("Duplicate/self-referential file inventory")
        expected[name] = item
    actual = {item["path"]: item for item in inventory(tree, exclude)}
    if set(actual) != set(expected):
        raise ValueError(f"File inventory differs: missing={sorted(set(expected) - set(actual))[:3]}, extra={sorted(set(actual) - set(expected))[:3]}")
    for name in expected:
        if actual[name] != expected[name]:
            raise ValueError(f"File integrity mismatch: {name}")


def project(root):
    return tomllib.loads((root / "Cargo.toml").read_text())["package"]


def verify_notices(notices, platform, root=ROOT):
    value = json.loads((notices / "manifest.json").read_text())
    if value.get("schema_version") != 1 or value.get("target") != TARGETS[platform]:
        raise ValueError("Rust notice target does not match package target")
    p = project(root)
    if value.get("application") != {"name": p["name"], "version": p["version"]} or value.get("cargo_lock_sha256") != digest(root / "Cargo.lock"):
        raise ValueError("Rust notices describe a stale application version/Cargo.lock")
    if value.get("dependency_edges") != ["normal", "build"] or value.get("dev_dependencies") is not False:
        raise ValueError("Rust notices do not identify the release dependency graph")
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    locked = {(p["name"], p["version"], p.get("source")): p.get("checksum") for p in lock["package"]}
    records, seen = [], set()
    for crate in value.get("crates", []):
        key = (crate["name"], crate["version"], crate["source"])
        if key in seen or locked.get(key) != crate["checksum_sha256"]:
            raise ValueError("Rust notice crate differs from locked dependency")
        seen.add(key)
        base = f"crates/{crate['name']}-{crate['version']}/"
        records.extend({**record, "path": base + record["path"]} for record in crate["files"])
    if not seen or value.get("crate_count") != len(seen):
        raise ValueError("Rust notice dependency inventory is empty/inconsistent")
    toolchain = value.get("toolchain", {})
    if not toolchain.get("rustc_version_verbose", "").startswith("rustc 1.99.0 "):
        raise ValueError("Rust toolchain notices must identify exact release toolchain 1.99.0")
    records.extend({**record, "path": "toolchain/" + record["path"]} for record in toolchain.get("files", []))
    if not any(record["path"] == "toolchain/COPYRIGHT-library.html" for record in records):
        raise ValueError("Rust standard-library notices missing")
    if digest(notices / "cargo-tree.txt") != value.get("cargo_tree_sha256"):
        raise ValueError("Cargo target tree integrity mismatch")
    verify_records(notices, records, exclude=("manifest.json", "README.txt", "cargo-tree.txt"))
    if not (notices / "README.txt").read_bytes().strip():
        raise ValueError("Rust notice README is empty")
    return value


def verify_source_binding(actual, expected):
    for key in ("source_set_id", "sources_lock_sha256", "builder_sha256", "source_archives"):
        if actual.get(key) != expected.get(key):
            raise ValueError(f"Corresponding source binding mismatch: {key}")


def verify_runtime(runtime, platform, root=ROOT):
    value = json.loads((runtime / "manifest.json").read_text())
    if value.get("schema_version") != 1 or value.get("provider") != "Nova source build" or value.get("platform") != platform:
        raise ValueError("Runtime must be Nova's verified native source build for this target")
    if value.get("native_runtime_check") != "PASS" or value.get("native_runtime_smoke", {}).get("status") != "PASS" or value.get("license") != "GPL-3.0-or-later" or value.get("patches") != []:
        raise ValueError("Runtime validation/licensing/patch gate is incomplete")
    verify_records(runtime, value["files"], exclude=("manifest.json",))
    recipe = root / "scripts/runtime-source"
    lock = json.loads((recipe / "sources.lock.json").read_text())
    build = json.loads((runtime / "build-manifest.json").read_text())
    binding = {"source_set_id": lock["source_set_id"], "sources_lock_sha256": digest(recipe / "sources.lock.json"), "builder_sha256": digest(recipe / "build_runtime.py"),
               "source_archives": [{"path": "archives/" + s["archive"], "sha256": s["sha256"], "bytes": s["archive_bytes"]} for s in lock["sources"]]}
    if build.get("schema_version") != 1 or build.get("platform") != platform or build.get("patches") != [] or build.get("native_runtime_smoke") != value.get("native_runtime_smoke"):
        raise ValueError("Runtime build manifest disagrees with native evidence")
    verify_source_binding({**build, "source_archives": binding["source_archives"]}, binding)
    if value.get("source_set_id") != lock["source_set_id"] or value.get("sources") != lock["sources"]:
        raise ValueError("Runtime source set differs from current locked source set")
    for name in source_bundle.RECIPE_FILES:
        if (runtime / "source" / name).read_bytes() != (recipe / name).read_bytes():
            raise ValueError(f"Runtime retained source recipe is stale: {name}")
    retained = json.loads((runtime / "source/source-manifest.json").read_text())
    if retained.get("archives") != binding["source_archives"] or retained.get("source_set_id") != lock["source_set_id"] or retained.get("patches") != []:
        raise ValueError("Runtime corresponding source archive inventory differs")
    for source in lock["sources"]:
        archive = runtime / "source/archives" / source["archive"]
        if archive.stat().st_size != source["archive_bytes"] or digest(archive) != source["sha256"]:
            raise ValueError("Runtime original source archive differs from lock")
        for item in source["license"]["file_hashes"]:
            license_path = runtime / "licenses" / source["name"] / safe_path(item["path"])
            if license_path.stat().st_size != item["bytes"] or digest(license_path) != item["sha256"]:
                raise ValueError("Runtime dependency license differs from locked upstream text")
    suffix = ".exe" if platform.startswith("windows-") else ""
    expected_bins = {"bin/ffmpeg" + suffix, "bin/ffprobe" + suffix}
    if {item["path"] for item in value["binary_files"]} != expected_bins or build.get("binary_files") != value["binary_files"]:
        raise ValueError("Runtime executable inventory differs from exact build")
    for item in value["binary_files"]:
        path = runtime / item["path"]
        if digest(path) != item["sha256"] or path.stat().st_size != item["bytes"] or builder.architecture(path, platform) != item["architecture"]:
            raise ValueError("Runtime executable bytes/architecture mismatch")
        if not item.get("dependencies") or any(not builder.allowed_dependency(dep, platform) for dep in item["dependencies"]):
            raise ValueError("Runtime has unapproved dynamic library imports")
    compiler = build.get("compiler_runtime", {})
    if platform != "macos-arm64" and (compiler.get("status") != "PASS" or not compiler.get("packages") or not compiler.get("support_files")):
        raise ValueError("Static GCC/MinGW support-code notices are incomplete")
    for owner in compiler.get("packages", {}).values():
        if not owner.get("license_files") or any(not (runtime / safe_path(name)).is_file() for name in owner["license_files"]):
            raise ValueError("Compiler runtime package license bytes missing")
    return value, binding


def application_imports(binary, platform):
    if platform == "macos-arm64":
        output = subprocess.check_output(["otool", "-L", str(binary)], text=True)
        imports = [line.strip().split(" (")[0] for line in output.splitlines()[1:]]
        approved = lambda item: item.startswith(("/usr/lib/", "/System/Library/"))
    elif platform == "linux-x86_64":
        output = subprocess.check_output(["readelf", "-d", str(binary)], text=True)
        if re.search(r"\((?:RPATH|RUNPATH)\)", output):
            raise ValueError("Application must not embed ELF RPATH/RUNPATH")
        imports = re.findall(r"\(NEEDED\).*\[(.*?)\]", output)
        system = builder.LINUX_SYSTEM_LIBRARIES | {"libgcc_s.so.1", "libstdc++.so.6", "libxkbcommon.so.0", "libwayland-client.so.0", "libX11.so.6", "libxcb.so.1"}
        approved = lambda item: item in system
    else:
        objdump = shutil.which("objdump") or "C:/msys64/ucrt64/bin/objdump.exe"
        output = subprocess.check_output([objdump, "-p", str(binary)], text=True)
        imports = re.findall(r"^\s*DLL Name:\s*(\S+)", output, re.M)
        extra = {"d3dcompiler_47.dll", "imm32.dll", "comdlg32.dll", "comctl32.dll", "dwmapi.dll", "hid.dll", "opengl32.dll", "rpcrt4.dll", "uxtheme.dll", "winspool.drv", "wsock32.dll", "cfgmgr32.dll", "propsys.dll", "shcore.dll", "uiautomationcore.dll", "powrprof.dll"}
        approved = lambda item: builder.allowed_dependency(item, platform) or item.lower() in extra
    if not imports or any(not approved(item) for item in imports):
        raise ValueError(f"Application imports unapproved external/system libraries: {imports}")
    return sorted(set(imports))


def verify_privacy(tree, private_tokens=None):
    if private_tokens is None:
        home = str(Path.home())
        private_tokens = [home.encode(), str(ROOT).encode(), home.replace("\\", "/").encode(), home.replace("\\", "\\\\").encode()]
    for record in inventory(tree):
        data = (tree / record["path"]).read_bytes()
        if any(token and token in data for token in private_tokens):
            raise ValueError(f"Public package contains a private local path: {record['path']}; rebuild/remap paths, do not redact upstream notices")


def diagnose(executable, version):
    environment = os.environ.copy()
    environment.pop("NOVA_RUNTIME", None)
    for key in list(environment):
        if key.startswith(("DYLD_", "LD_")):
            environment.pop(key)
    result = subprocess.run([str(executable), "--diagnose"], cwd=executable.parent, env=environment, check=True, capture_output=True, text=True, timeout=60)
    value = json.loads(result.stdout)
    if value.get("ready") is not True or value.get("app_version") != version or value.get("error") is not None:
        raise ValueError("Staged application is not ready or has a stale compiled version")
    return value


def paths_for(stage, platform):
    if platform == "macos-arm64":
        contents = stage / "Nova Flux.app/Contents"
        return contents / "MacOS/nova-flux", contents / "Resources/runtime", contents / "Resources/licenses"
    return stage / ("nova-flux.exe" if platform.startswith("windows-") else "nova-flux"), stage / "runtime", stage / "licenses"


def extract_checked(archive, destination, top):
    seen, total = set(), 0
    destination.mkdir(parents=True, exist_ok=False)
    def target(name, size, mode):
        nonlocal total
        relative = safe_path(name)
        if relative.parts[0] != top or len(relative.parts) < 2 or name.casefold() in seen:
            raise ValueError("Archive has duplicate paths or an unexpected root")
        seen.add(name.casefold())
        total += size
        if total > 2 * 1024**3 or len(seen) > 50000:
            raise ValueError("Package exceeds extraction limits")
        path = destination / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        return path
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive) as source:
            for entry in source.infolist():
                if entry.is_dir() or stat.S_ISLNK(entry.external_attr >> 16):
                    raise ValueError("Unexpected ZIP directory/link")
                path = target(entry.filename, entry.file_size, entry.external_attr >> 16)
                path.write_bytes(source.read(entry))
                path.chmod(0o755 if (entry.external_attr >> 16) & 0o111 else 0o644)
    else:
        with tarfile.open(archive, "r:gz") as source:
            for entry in source:
                if not entry.isfile():
                    raise ValueError("Unexpected tar link/directory/special file")
                path = target(entry.name, entry.size, entry.mode)
                with source.extractfile(entry) as inp, path.open("xb") as out:
                    shutil.copyfileobj(inp, out)
                path.chmod(0o755 if entry.mode & 0o111 else 0o644)
    return destination / top


def write_archive(stage, archive):
    files = inventory(stage)
    if archive.name.endswith(".zip"):
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as output:
            for record in files:
                path = stage / record["path"]
                info = zipfile.ZipInfo(stage.name + "/" + record["path"], date_time=(1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.external_attr = (stat.S_IFREG | (0o755 if path.stat().st_mode & 0o111 else 0o644)) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                with output.open(info, "w") as out, path.open("rb") as inp:
                    shutil.copyfileobj(inp, out)
    else:
        with tarfile.open(archive, "w:gz", compresslevel=6) as output:
            for record in files:
                path = stage / record["path"]
                info = tarfile.TarInfo(stage.name + "/" + record["path"])
                info.size = path.stat().st_size
                info.mode = 0o755 if path.stat().st_mode & 0o111 else 0o644
                info.uid = info.gid = info.mtime = 0
                info.uname = info.gname = ""
                with path.open("rb") as inp:
                    output.addfile(info, inp)


def verify_package_tree(stage):
    value = json.loads((stage / MANIFEST).read_text())
    if value.get("schema_version") != 1:
        raise ValueError("Unsupported public package manifest")
    verify_records(stage, value["files"], exclude=(MANIFEST,))
    return value


def package(platform, runtime, notices, binary, source_zip, destination, root=ROOT):
    expected_host = {"macos-arm64": ("Darwin", {"arm64", "aarch64"}), "windows-x86_64": ("Windows", {"amd64", "x86_64"}), "linux-x86_64": ("Linux", {"amd64", "x86_64"})}[platform]
    if native_platform.system() != expected_host[0] or native_platform.machine().lower() not in expected_host[1]:
        raise ValueError(f"Native {platform} host required for verified packaging")
    for item in (runtime, notices, binary, source_zip):
        if item.is_symlink():
            raise ValueError("Release inputs cannot be symlinks")
    runtime, notices, binary, source_zip = (path.resolve(strict=True) for path in (runtime, notices, binary, source_zip))
    destination = destination.absolute()
    if destination.exists() or destination.is_symlink():
        raise ValueError("Refusing to overwrite existing release destination")
    if any(input_dir == destination.resolve() or input_dir in destination.resolve().parents for input_dir in (runtime, notices)):
        raise ValueError("Release destination must be outside runtime/notices inputs")
    p = project(root)
    version = p["version"]
    runtime_value, binding = verify_runtime(runtime, platform, root)
    notice_value = verify_notices(notices, platform, root)
    source_value = source_bundle.verify_zip(source_zip)
    verify_source_binding(source_value, binding)
    builder.architecture(binary, platform)
    imports = application_imports(binary, platform)
    input_binary_sha = digest(binary)
    source_sha = digest(source_zip)
    source_url = f"{SOURCE_RELEASE}/v{version}/{urllib.parse.quote(source_zip.name)}"
    package_name = f"Nova-Flux-{version}-{platform}"
    destination.mkdir(parents=True, exist_ok=False)
    with tempfile.TemporaryDirectory(prefix=".nova-verified-", dir=destination) as temporary:
        scratch = Path(temporary)
        stage = scratch / package_name
        stage.mkdir()
        executable, staged_runtime, licenses = paths_for(stage, platform)
        executable.parent.mkdir(parents=True, exist_ok=True)
        licenses.mkdir(parents=True)
        shutil.copy2(binary, executable)
        executable.chmod(0o755)
        shutil.copytree(runtime, staged_runtime)
        shutil.copytree(notices, licenses / "rust")
        shutil.copyfile(root / "LICENSE", licenses / "application-LICENSE.txt")
        notice_text = (f"Nova Flux {version} — {platform}\n"
                       + "Nova's application code is MIT licensed; see application-LICENSE.txt.\n"
                       + "Original Rust and embedded font notices are in rust/.\n"
                       + "The separate FFmpeg/ffprobe runtime is GPL version 3 or later.\n"
                       + "Its complete original codec sources and exact build instructions are\n"
                       + "included inside runtime/source/; dependency notices are in runtime/licenses/.\n"
                       + f"The separately published complete corresponding-source ZIP is:\n{source_url}\nSHA-256: {source_sha}\n"
                       + "Keep the complete application folder when extracting.\n"
                       + ("This macOS app has a local ad-hoc signature and is not notarized by Apple.\n" if platform == "macos-arm64" else "")
                       + "Native package diagnosis does not establish GUI/GPU behavior on every OS.\n")
        (licenses / "README.txt").write_text(notice_text)
        (stage / "README.txt").write_text(notice_text)
        if platform == "macos-arm64":
            contents = stage / "Nova Flux.app/Contents"
            shutil.copyfile(root / "assets/AppIcon.icns", contents / "Resources/AppIcon.icns")
            info = {"CFBundleName": "Nova Flux", "CFBundleDisplayName": "Nova Flux", "CFBundleExecutable": "nova-flux", "CFBundleIdentifier": "com.nova.flux", "CFBundlePackageType": "APPL", "CFBundleShortVersionString": version, "CFBundleVersion": "7", "CFBundleIconFile": "AppIcon.icns", "CFBundleDevelopmentRegion": "en", "CFBundleLocalizations": ["en"], "LSMinimumSystemVersion": "12.0", "LSApplicationCategoryType": "public.app-category.video", "NSHighResolutionCapable": True}
            (contents / "Info.plist").write_bytes(plistlib.dumps(info))
            subprocess.run(["codesign", "--force", "--sign", "-", str(contents.parent)], check=True, capture_output=True)
            subprocess.run(["codesign", "--verify", "--deep", "--strict", str(contents.parent)], check=True, capture_output=True)
        else:
            shutil.copyfile(root / "assets/icon.png", stage / "icon.png")
        verify_runtime(staged_runtime, platform, root)  # Signing must not alter media executables.
        diagnose_value = diagnose(executable, version)
        verify_privacy(stage)
        manifest = {"schema_version": 1, "application": "Nova Flux", "version": version, "platform": platform, "application_executable": executable.relative_to(stage).as_posix(), "runtime_directory": staged_runtime.relative_to(stage).as_posix(), "license_directory": licenses.relative_to(stage).as_posix(), "distribution_status": "verified-source-runtime-package", "cargo_lock_sha256": digest(root / "Cargo.lock"), "input_application_sha256": input_binary_sha, "packaged_application_sha256": digest(executable), "application_dynamic_imports": imports, "runtime_manifest_sha256": digest(staged_runtime / "manifest.json"), "rust_notices_manifest_sha256": digest(licenses / "rust/manifest.json"), "source_bundle": {"filename": source_zip.name, "sha256": source_sha, "url": source_url, **binding}, "app_diagnose": diagnose_value, "runtime_native_smoke": runtime_value["native_runtime_smoke"], "files": inventory(stage)}
        (stage / MANIFEST).write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        verify_package_tree(stage)
        suffix = ".tar.gz" if platform.startswith("linux-") else ".zip"
        archive = scratch / (package_name + suffix)
        write_archive(stage, archive)
        extracted = extract_checked(archive, scratch / "extracted", package_name)
        verify_package_tree(extracted)
        extracted_exe, extracted_runtime, extracted_licenses = paths_for(extracted, platform)
        verify_runtime(extracted_runtime, platform, root)
        verify_notices(extracted_licenses / "rust", platform, root)
        diagnose(extracted_exe, version)
        if application_imports(extracted_exe, platform) != imports:
            raise ValueError("Extracted application dynamic imports changed")
        verify_privacy(extracted)
        if platform == "macos-arm64":
            subprocess.run(["codesign", "--verify", "--deep", "--strict", str(extracted / "Nova Flux.app")], check=True, capture_output=True)
        if digest(binary) != input_binary_sha or digest(source_zip) != source_sha:
            raise ValueError("Release input changed while packaging")
        final_archive = destination / archive.name
        digest_value = digest(archive)
        stage.rename(destination / stage.name)
        archive.rename(final_archive)
        final_archive.with_name(final_archive.name + ".sha256").write_text(digest_value + "  " + final_archive.name + "\n", encoding="ascii")
    return {"archive": str(final_archive), "sha256": digest_value, "package_directory": str(destination / package_name), "version": version, "platform": platform, "status": "PASS", "scope": "Source/runtime/notices integrity, native packaged and extracted diagnosis; GUI acceptance is separate"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=tuple(TARGETS), required=True)
    for name in ("runtime", "notices", "binary", "source-bundle", "destination"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(package(args.platform, args.runtime, args.notices, args.binary, args.source_bundle, args.destination), indent=2))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, subprocess.SubprocessError, zipfile.BadZipFile, tarfile.TarError) as error:
        raise SystemExit(f"Verified packaging failed: {error}")
