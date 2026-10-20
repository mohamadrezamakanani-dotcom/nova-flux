#!/usr/bin/env python3
"""Build Nova's pinned FFmpeg runtime natively; never acquire binary runtimes.

Only Python's standard library is needed by this driver. See BUILD.md for the
native compiler/build tools and the distinction between building and acceptance.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shlex
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import time

HERE = Path(__file__).resolve().parent
PLATFORMS = ("macos-arm64", "linux-x86_64", "windows-x86_64")
ORDER = ("zlib", "libogg", "libvorbis", "opus", "lame", "x264", "libvpx", "dav1d", "ffmpeg")
# In upstream libvpx, unversioned arm64-darwin-gcc selects iOS. Darwin 20
# identifies desktop macOS 11; MACOSX_DEPLOYMENT_TARGET sets the minimum OS.
VPX_TARGETS = {"macos-arm64": "arm64-darwin20-gcc", "linux-x86_64": "x86_64-linux-gcc", "windows-x86_64": "x86_64-win64-gcc"}
# libvpx embeds every configure argument in vpx_codec_build_config(). The
# relative prefix resolves identically from sibling build/libvpx and
# build/ffmpeg directories, including when pkg-config reads installed vpx.pc.
VPX_INSTALL_PREFIX = "../../prefix"
REQUIRED_ENCODERS = {"libx264", "mpeg4", "libvpx-vp9", "aac", "libmp3lame", "libopus", "libvorbis", "flac", "pcm_s16le", "gif"}
REQUIRED_FILTERS = {"scale", "pad", "fps", "palettegen", "paletteuse", "testsrc", "sine", "aresample"}
LINUX_SYSTEM_LIBRARIES = {
    "libc.so.6", "libm.so.6", "libpthread.so.0", "libdl.so.2", "librt.so.1",
    "libutil.so.1", "libresolv.so.2", "ld-linux-x86-64.so.2",
}
WINDOWS_SYSTEM_DLLS = {
    "advapi32.dll", "avicap32.dll", "bcrypt.dll", "crypt32.dll", "d3d11.dll",
    "dxgi.dll", "dxva2.dll", "gdi32.dll", "kernel32.dll", "ksuser.dll",
    "mf.dll", "mfplat.dll", "mfreadwrite.dll", "mfuuid.dll", "msvcrt.dll",
    "ntdll.dll", "ole32.dll", "oleaut32.dll", "psapi.dll", "secur32.dll",
    "setupapi.dll", "shell32.dll", "shlwapi.dll", "strmiids.dll", "ucrtbase.dll",
    "user32.dll", "userenv.dll", "version.dll", "vfw32.dll", "winmm.dll",
    "ws2_32.dll", "wtsapi32.dll",
}
CONTROLLED_ENV = (
    "CC", "CXX", "AR", "RANLIB", "NM", "STRIP", "CFLAGS", "CXXFLAGS",
    "CPPFLAGS", "LDFLAGS", "PKG_CONFIG", "PKG_CONFIG_PATH", "PKG_CONFIG_LIBDIR",
    "SOURCE_DATE_EPOCH", "MACOSX_DEPLOYMENT_TARGET", "LC_ALL", "LANG", "TZ",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def require_host(target: str) -> None:
    expected = {"macos-arm64": ("Darwin", {"arm64", "aarch64"}),
                "linux-x86_64": ("Linux", {"x86_64", "amd64"}),
                "windows-x86_64": ("Windows", {"x86_64", "amd64"})}[target]
    if platform.system() != expected[0] or platform.machine().lower() not in expected[1]:
        raise ValueError(f"Native {target} host required; found {platform.system()} {platform.machine()}")
    if target.startswith("windows-") and os.environ.get("MSYSTEM") != "UCRT64":
        raise ValueError("Start the native Windows build in an MSYS2 UCRT64 shell (MSYSTEM=UCRT64)")


def safe_component(value: str) -> str:
    if not value or value in (".", "..") or not re.fullmatch(r"[A-Za-z0-9_.+\-]+", value):
        raise ValueError(f"Unsafe source path component: {value!r}")
    return value


def load_lock(path: Path, cache: Path) -> dict:
    lock = json.loads(path.read_text(encoding="utf-8"))
    entries = lock.get("sources", [])
    if lock.get("schema_version") != 1 or {s["name"] for s in entries} != set(ORDER) or len(entries) != len(ORDER):
        raise ValueError("Source lock must contain exactly the nine supported source packages")
    if lock.get("patches") or any(s.get("patches") for s in entries):
        raise ValueError("This recipe requires unmodified sources. Add explicit patch verification/application before extending the lock")
    for entry in entries:
        for key in ("name", "archive", "archive_root", "source_dir"):
            safe_component(entry[key])
        archive = cache / "archives" / entry["archive"]
        if not archive.is_file() or archive.is_symlink():
            raise ValueError(f"Missing regular source archive: {archive}")
        if archive.stat().st_size != entry["archive_bytes"] or sha256(archive) != entry["sha256"]:
            raise ValueError(f"Source archive does not match the lock: {entry['archive']}")
    return lock


def extract_source(archive: Path, destination: Path, archive_root: str) -> None:
    """Extract exact archive content, omitting only its declared top directory.

    All nine locked archives contain regular files/directories only. Refusing
    links makes accidental escapes and platform-specific symlink loss explicit.
    """
    destination.mkdir(parents=True, exist_ok=False)
    seen: set[str] = set()
    total = 0
    with tarfile.open(archive, "r:*") as source:
        for member in source:
            raw = member.name.rstrip("/")
            parts = raw.split("/")
            if ("\\" in raw or "\x00" in raw or any(p in ("", ".", "..") or ":" in p for p in parts)
                    or not parts or parts[0] != archive_root or not (member.isfile() or member.isdir())):
                raise ValueError(f"Unsafe or unexpected source archive member: {member.name!r}")
            if len(parts) == 1:
                if not member.isdir():
                    raise ValueError("Source archive root must be a directory")
                continue
            relative = "/".join(parts[1:])
            if relative.casefold() in seen:
                raise ValueError(f"Case-colliding source archive entry: {relative}")
            seen.add(relative.casefold())
            total += member.size
            if total > 1024**3:
                raise ValueError("Source archive exceeds the 1 GiB extracted-size budget")
            target = destination.joinpath(*parts[1:])
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as inp, target.open("xb") as out:
                    remaining = member.size
                    while remaining:
                        block = inp.read(min(remaining, 1024 * 1024))
                        if not block:
                            raise ValueError("Truncated source archive")
                        out.write(block)
                        remaining -= len(block)
                target.chmod(0o755 if member.mode & 0o111 else 0o644)


def file_manifest(root: Path, excludes: tuple[str, ...] = ()) -> list[dict]:
    result = []
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix()
        if path.is_symlink():
            raise ValueError(f"Unexpected symlink in runtime: {relative}")
        if path.is_file() and relative not in excludes:
            result.append({"path": relative, "bytes": path.stat().st_size, "sha256": sha256(path)})
    return result


def architecture(path: Path, target: str) -> str:
    with path.open("rb") as stream:
        header = stream.read(64)
        if target == "macos-arm64":
            if header[:4] != b"\xcf\xfa\xed\xfe" or struct.unpack_from("<I", header, 4)[0] != 0x0100000C:
                raise ValueError(f"Expected thin arm64 Mach-O: {path.name}")
            return "macho-arm64"
        if target == "linux-x86_64":
            if header[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", header, 18)[0] != 62:
                raise ValueError(f"Expected little-endian x86_64 ELF: {path.name}")
            return "elf-x86_64"
        if header[:2] != b"MZ" or len(header) != 64:
            raise ValueError(f"Expected x86_64 PE32+: {path.name}")
        stream.seek(struct.unpack_from("<I", header, 60)[0])
        pe = stream.read(26)
        if len(pe) != 26 or pe[:4] != b"PE\0\0" or struct.unpack_from("<H", pe, 4)[0] != 0x8664 or struct.unpack_from("<H", pe, 24)[0] != 0x20B:
            raise ValueError(f"Expected x86_64 PE32+: {path.name}")
        return "pe-x86_64"


def allowed_dependency(name: str, target: str) -> bool:
    if target == "macos-arm64":
        return name.startswith(("/usr/lib/", "/System/Library/"))
    if target == "linux-x86_64":
        return name in LINUX_SYSTEM_LIBRARIES
    lower = name.lower()
    return lower in WINDOWS_SYSTEM_DLLS or bool(re.fullmatch(r"(?:api|ext)-ms-win-[a-z0-9-]+\.dll", lower))


class Builder:
    def __init__(self, args: argparse.Namespace):
        self.args = args
        self.target = args.platform
        self.work = args.work_dir.resolve()
        self.cache = args.sources.resolve()
        self.out = args.destination.resolve()
        self.lock_path = args.lock.resolve()
        self.recipe_files = {name: (HERE / name).read_bytes() for name in ("build_runtime.py", "BUILD.md", "acquire_sources.py")}
        self.recipe_files["sources.lock.json"] = self.lock_path.read_bytes()
        self.lock = load_lock(self.lock_path, self.cache)
        if self.lock_path.read_bytes() != self.recipe_files["sources.lock.json"]:
            raise ValueError("Source lock changed during initialization")
        self.entries = {entry["name"]: entry for entry in self.lock["sources"]}
        self.prefix = self.work / "prefix"
        self.raw_logs = self.work / "logs"
        self.commands: list[dict] = []
        self.tools: dict = {}
        self.operations: list[dict] = []
        self.compiler_runtime: dict = {}
        self.is_windows = self.target == "windows-x86_64"
        self.cc = "clang" if self.target == "macos-arm64" else "gcc"
        self.cxx = "clang++" if self.target == "macos-arm64" else "g++"
        self.replacements = sorted(((str(self.work), "{WORK}"), (str(self.cache), "{SOURCES}"),
                                    (str(self.out), "{RUNTIME}"), (str(HERE), "{RECIPE}"),
                                    (str(Path.home()), "{HOME}")), key=lambda x: -len(x[0]))
        if self.is_windows:
            self.replacements += [(self.posix(Path(path)), token) for path, token in self.replacements[:]]
            self.replacements.sort(key=lambda item: -len(item[0]))
        self.env = self.build_environment()

    def public(self, value: str) -> str:
        for original, token in self.replacements:
            for form in {original, original.replace("\\", "/"), original.replace("\\", "\\\\")}:
                value = value.replace(form, token)
        return value

    def posix(self, path: Path) -> str:
        if not self.is_windows:
            return path.as_posix()
        result = subprocess.run(["cygpath", "-u", str(path)], check=True, capture_output=True, text=True)
        return result.stdout.strip()

    def build_environment(self) -> dict[str, str]:
        env = dict(os.environ)
        for key in list(env):
            if key in {"CC", "CXX", "AR", "RANLIB", "NM", "STRIP", "LD", "AS", "CFLAGS", "CXXFLAGS", "CPPFLAGS", "LDFLAGS", "LIBS", "LDEXEFLAGS", "CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH", "LIBRARY_PATH", "SDKROOT", "CMAKE_PREFIX_PATH", "CONFIG_SITE", "MAKEFLAGS", "MFLAGS", "GNUMAKEFLAGS", "PKG_CONFIG_SYSROOT_DIR", "PKG_CONFIG_TOP_BUILD_DIR"} or key.startswith(("DYLD_", "LD_", "PKG_CONFIG_")):
                env.pop(key, None)
        # No CPU=native: distributed x86 binaries retain runtime CPU dispatch.
        mapped = self.work.as_posix()
        flags = f"-O2 -ffile-prefix-map={mapped}=. -fdebug-prefix-map={mapped}=."
        if self.is_windows:
            # Both path forms can appear in __FILE__ depending on which build
            # tool invokes GCC. Neither map is passed as a configure argument.
            unix = self.posix(self.work)
            flags += f" -ffile-prefix-map={unix}=. -fdebug-prefix-map={unix}=."
        if self.target == "linux-x86_64":
            flags += " -march=x86-64 -mtune=generic"
        env.update({"CC": self.cc, "CXX": self.cxx, "AR": "ar", "RANLIB": "ranlib", "NM": "nm", "STRIP": "strip",
                    "CFLAGS": flags, "CXXFLAGS": flags, "CPPFLAGS": "", "LDFLAGS": "",
                    "PKG_CONFIG": "pkg-config", "PKG_CONFIG_PATH": "",
                    "PKG_CONFIG_LIBDIR": os.pathsep.join(str(self.prefix / p) for p in ("lib/pkgconfig", "share/pkgconfig")),
                    "LC_ALL": "C", "LANG": "C", "TZ": "UTC", "SOURCE_DATE_EPOCH": "1792454400"})
        if self.target == "macos-arm64":
            env["MACOSX_DEPLOYMENT_TARGET"] = "11.0"
        if self.is_windows:
            env["LDFLAGS"] = "-static -static-libgcc -static-libstdc++"
        elif self.target == "linux-x86_64":
            env["LDFLAGS"] = "-static-libgcc -static-libstdc++"
        return env

    def run(self, package: str, args: list[str], cwd: Path, *, capture: bool = False, timeout: int = 3600) -> str:
        record = {"package": package, "argv": [self.public(str(x)) for x in args], "cwd": self.public(str(cwd))}
        self.commands.append(record)
        if self.args.dry_run:
            return ""
        number = len(self.commands)
        log_path = self.raw_logs / f"{number:03d}-{package}.log"
        record["log"] = f"provenance/logs/{log_path.name}"
        started = time.monotonic()
        with log_path.open("w", encoding="utf-8") as log:
            log.write(shlex.join(args) + "\n")
            log.flush()
            if capture:
                process = subprocess.run(args, cwd=cwd, env=self.env, capture_output=True, text=True,
                                         encoding="utf-8", errors="replace", timeout=timeout)
                output = process.stdout
                log.write(process.stdout + process.stderr)
            else:
                process = subprocess.run(args, cwd=cwd, env=self.env, stdout=log, stderr=subprocess.STDOUT, timeout=timeout)
                output = ""
        record.update({"exit_code": process.returncode, "elapsed_seconds": round(time.monotonic() - started, 3)})
        if process.returncode:
            tail = "\n".join(log_path.read_text(encoding="utf-8", errors="replace").splitlines()[-35:])
            raise RuntimeError(f"{package} failed (exit {process.returncode}); full log: {log_path}\n{tail}")
        return output

    def preflight(self) -> None:
        require_host(self.target)
        for path in (self.work, self.cache, self.out):
            if any(character.isspace() or character in "#$'\"`" for character in str(path)):
                raise ValueError("Use build/cache/output directories without whitespace or shell/make metacharacters")
        required = [self.cc, self.cxx, "ar", "ranlib", "nm", "strip", "bash", "make", "pkg-config", "cmake", "meson", "ninja"]
        required += ["otool", "codesign"] if self.target == "macos-arm64" else ["objdump", "nasm"] if self.is_windows else ["readelf", "nasm"]
        if self.is_windows:
            required += ["cygpath", "pacman"]
        elif self.target == "linux-x86_64" and not shutil.which("dpkg-query") and not shutil.which("rpm"):
            raise ValueError("Linux compiler notices require a package-managed GCC (dpkg-query or rpm)")
        missing = [tool for tool in required if not shutil.which(tool)]
        if missing:
            raise ValueError("Missing native build tools: " + ", ".join(missing))
        for tool in required:
            args = [tool, "--version"] if tool not in ("otool", "codesign") else [tool, "-h"]
            result = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", errors="replace", env=self.env, timeout=20)
            self.tools[tool] = {"executable": self.public(shutil.which(tool)),
                                "version_output": self.public((result.stdout + result.stderr).strip())[:12000]}
        self.tools["python"] = {"version_output": sys.version, "implementation": platform.python_implementation()}
        machine = subprocess.run([self.cc, "-dumpmachine"], check=True, capture_output=True, text=True, env=self.env).stdout.strip()
        if self.is_windows and machine != "x86_64-w64-mingw32":
            raise ValueError(f"Expected native UCRT64 MinGW GCC; compiler target is {machine!r}")
        self.tools[self.cc]["target"] = machine
        if self.is_windows and "ucrt64" not in shutil.which(self.cc).replace("\\", "/").lower():
            raise ValueError("Windows GCC must come from the MSYS2 UCRT64 prefix")

    def source(self, name: str) -> Path:
        return self.work / "src" / self.entries[name]["source_dir"]

    def build(self, name: str) -> Path:
        return self.work / "build" / name

    def prepare(self) -> None:
        if self.out.exists():
            raise ValueError(f"Destination already exists; preserve it and choose another path: {self.out}")
        if self.work.exists() and any(self.work.iterdir()):
            raise ValueError(f"Build directory must be empty; preserve earlier work and choose another path: {self.work}")
        if self.args.dry_run:
            return
        self.raw_logs.mkdir(parents=True)
        self.prefix.mkdir()
        for name, content in self.recipe_files.items():
            (self.work / "build-inputs").mkdir(exist_ok=True)
            (self.work / "build-inputs" / name).write_bytes(content)
        for name in ORDER:
            entry = self.entries[name]
            print(f"Extracting verified source: {name} {entry['version']}", flush=True)
            extract_source(self.cache / "archives" / entry["archive"], self.source(name), entry["archive_root"])
            self.build(name).mkdir(parents=True)

    def cmake(self, name: str, options: list[str]) -> None:
        args = ["cmake", "-S", str(self.source(name)), "-B", str(self.build(name)), "-G", "Ninja",
                "-DCMAKE_BUILD_TYPE=Release", f"-DCMAKE_INSTALL_PREFIX={self.prefix.as_posix()}", "-DCMAKE_INSTALL_LIBDIR=lib",
                "-DBUILD_SHARED_LIBS=OFF", "-DBUILD_TESTING=OFF", "-DCMAKE_POSITION_INDEPENDENT_CODE=ON",
                "-DCMAKE_POLICY_VERSION_MINIMUM=3.5", f"-DCMAKE_PREFIX_PATH={self.prefix.as_posix()}",
                "-DCMAKE_FIND_USE_PACKAGE_REGISTRY=OFF", "-DCMAKE_FIND_USE_SYSTEM_PACKAGE_REGISTRY=OFF"]
        if self.target == "macos-arm64":
            args += ["-DCMAKE_OSX_ARCHITECTURES=arm64", "-DCMAKE_OSX_DEPLOYMENT_TARGET=11.0"]
        self.run(name, args + options, self.work)
        self.run(name, ["cmake", "--build", str(self.build(name)), "--parallel", str(self.args.jobs)], self.work)
        self.run(name, ["cmake", "--install", str(self.build(name))], self.work)

    def configure_make(self, name: str, options: list[str], *, in_source: bool = False) -> None:
        cwd = self.source(name) if in_source else self.build(name)
        script = self.source(name) / "configure"
        self.run(name, ["bash", self.posix(script), *options], cwd)
        self.run(name, ["make", f"-j{self.args.jobs}"], cwd)
        if name != "ffmpeg":
            self.run(name, ["make", "install"], cwd)

    def recipes(self) -> None:
        prefix = self.posix(self.prefix)
        self.cmake("zlib", ["-DZLIB_BUILD_SHARED=OFF", "-DZLIB_BUILD_STATIC=ON", "-DZLIB_BUILD_TESTING=OFF", "-DZLIB_BUILD_MINIZIP=OFF"])
        if self.is_windows:
            # Upstream Windows static target uses suffix 's'; FFmpeg's zlib
            # check and upstream zlib.pc both use -lz. Preserve exact bytes.
            operation = {"operation": "copy-static-library-alias", "from": "{WORK}/prefix/lib/libzs.a", "to": "{WORK}/prefix/lib/libz.a", "reason": "zlib 1.3.2 static Windows name versus -lz consumer convention"}
            self.operations.append(operation)
            if not self.args.dry_run:
                shutil.copyfile(self.prefix / "lib" / "libzs.a", self.prefix / "lib" / "libz.a")
        self.cmake("libogg", ["-DINSTALL_DOCS=OFF", "-DINSTALL_PKG_CONFIG_MODULE=ON", "-DINSTALL_CMAKE_PACKAGE_MODULE=ON"])
        self.cmake("libvorbis", [f"-DOGG_INCLUDE_DIR={self.prefix.as_posix()}/include", f"-DOGG_LIBRARY={self.prefix.as_posix()}/lib/libogg.a"])
        self.cmake("opus", ["-DOPUS_BUILD_SHARED_LIBRARY=OFF", "-DOPUS_BUILD_TESTING=OFF", "-DOPUS_BUILD_PROGRAMS=OFF", "-DOPUS_INSTALL_PKG_CONFIG_MODULE=ON", "-DOPUS_DRED=OFF", "-DOPUS_OSCE=OFF"])
        self.configure_make("lame", [f"--prefix={prefix}", "--libdir=" + prefix + "/lib", "--disable-shared", "--enable-static", "--with-pic", "--disable-frontend", "--disable-decoder", "--disable-analyzer-hooks"])
        # x264's own configure defaults to no shared library and enables its
        # native Windows thread implementation without a positive flag.
        x264 = [f"--prefix={prefix}", "--libdir=" + prefix + "/lib", "--enable-static", "--enable-pic", "--disable-cli", "--disable-opencl", "--disable-lavf", "--disable-swscale", "--disable-ffms"]
        self.configure_make("x264", x264)
        vpx_target = VPX_TARGETS[self.target]
        self.configure_make("libvpx", [f"--prefix={VPX_INSTALL_PREFIX}", "--libdir=" + VPX_INSTALL_PREFIX + "/lib", f"--target={vpx_target}", "--enable-static", "--disable-shared", "--enable-pic", "--enable-vp9-highbitdepth", "--disable-examples", "--disable-tools", "--disable-docs", "--disable-unit-tests", "--disable-install-srcs"])
        self.run("dav1d", ["meson", "setup", str(self.build("dav1d")), str(self.source("dav1d")),
                           "--prefix=" + self.prefix.as_posix(), "--libdir=lib", "--buildtype=release", "--default-library=static", "--wrap-mode=nodownload",
                           "-Denable_tools=false", "-Denable_tests=false", "-Denable_examples=false", "-Denable_docs=false", "-Dxxhash_muxer=disabled"], self.work)
        self.run("dav1d", ["meson", "compile", "-C", str(self.build("dav1d")), "-j", str(self.args.jobs)], self.work)
        self.run("dav1d", ["meson", "install", "-C", str(self.build("dav1d"))], self.work)
        self.assert_static_prefix()
        # These paths stay relative in FFmpeg's publicly visible -version
        # configuration. Absolute prefix maps are environment flags only.
        extra_ldflags = "-L../../prefix/lib"
        if self.is_windows:
            extra_ldflags += " -static -static-libgcc -static-libstdc++"
        elif self.target == "linux-x86_64":
            extra_ldflags += " -static-libgcc -static-libstdc++"
        options = ["--prefix=/nova-runtime", "--enable-gpl", "--enable-version3", "--disable-nonfree", "--disable-autodetect", "--disable-network",
                   "--disable-shared", "--enable-static", "--enable-pic", "--disable-debug", "--disable-doc", "--disable-ffplay", "--enable-ffmpeg", "--enable-ffprobe",
                   "--enable-libx264", "--enable-libmp3lame", "--enable-libopus", "--enable-libvorbis", "--enable-libvpx", "--enable-libdav1d", "--enable-zlib",
                   "--pkg-config=pkg-config", "--pkg-config-flags=--static", "--cc=" + self.cc, "--cxx=" + self.cxx,
                   "--extra-cflags=-I../../prefix/include", "--extra-ldflags=" + extra_ldflags]
        if self.target == "macos-arm64":
            options += ["--arch=aarch64", "--target-os=darwin", "--enable-videotoolbox"]
        elif self.is_windows:
            options += ["--arch=x86_64", "--target-os=mingw32"]
        else:
            options += ["--arch=x86_64", "--target-os=linux"]
        self.configure_make("ffmpeg", options)

    def assert_static_prefix(self) -> None:
        if self.args.dry_run:
            return
        unexpected = [p for p in self.prefix.rglob("*") if p.is_file() and (p.suffix.lower() in {".dll", ".dylib"} or ".so" in p.name or p.name.endswith(".dll.a"))]
        if unexpected:
            raise ValueError("A dependency installed shared libraries: " + ", ".join(p.name for p in unexpected))
        expected = ("libz.a", "libogg.a", "libvorbis.a", "libvorbisenc.a", "libopus.a", "libmp3lame.a", "libx264.a", "libvpx.a", "libdav1d.a")
        missing = [name for name in expected if not (self.prefix / "lib" / name).is_file()]
        if missing:
            raise ValueError("Missing expected static dependency libraries: " + ", ".join(missing))

    def dependencies(self, binary: Path) -> list[str]:
        if self.target == "macos-arm64":
            output = self.run("audit", ["otool", "-L", str(binary)], self.work, capture=True)
            deps = [line.strip().split(" (")[0] for line in output.splitlines()[1:]]
        elif self.is_windows:
            output = self.run("audit", ["objdump", "-p", str(binary)], self.work, capture=True)
            deps = re.findall(r"^\s*DLL Name:\s*(\S+)", output, re.M)
        else:
            output = self.run("audit", ["readelf", "-d", str(binary)], self.work, capture=True)
            deps = re.findall(r"\(NEEDED\).*\[(.*?)\]", output)
            if re.search(r"\((?:RPATH|RUNPATH)\)", output):
                raise ValueError("Runtime must not embed ELF RPATH/RUNPATH")
        if not deps:
            raise ValueError(f"Unable to inspect dynamic dependencies: {binary.name}")
        unexpected = [dep for dep in deps if not allowed_dependency(dep, self.target)]
        if unexpected:
            raise ValueError(f"Unexpected external runtime dependencies in {binary.name}: {unexpected}")
        return deps

    def runtime_run(self, tool: str, args: list[str], *, cwd: Path | None = None) -> str:
        suffix = ".exe" if self.is_windows else ""
        return self.run("runtime-check", [str(self.out / "bin" / (tool + suffix)), *args], cwd or self.work, capture=True, timeout=120)

    def smoke(self) -> dict:
        smoke = self.work / "smoke"
        smoke.mkdir()
        common = ["-nostdin", "-hide_banner", "-loglevel", "error", "-y"]
        self.runtime_run("ffmpeg", common + ["-f", "lavfi", "-i", "testsrc=size=193x129:rate=12:duration=1.25", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=1.25", "-c:v", "libx264", "-pix_fmt", "yuv444p", "-c:a", "aac", "-shortest", "odd-input.mp4"], cwd=smoke)
        video = ["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac"]
        cases = [("video." + ext, video, {"h264", "aac"}) for ext in ("mp4", "mov", "mkv")]
        cases += [
            ("video.webm", ["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libvpx-vp9", "-deadline", "realtime", "-cpu-used", "8", "-c:a", "libopus"], {"vp9", "opus"}),
            ("video.avi", ["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "mpeg4", "-c:a", "libmp3lame"], {"mpeg4", "mp3"}),
            ("audio.mp3", ["-vn", "-c:a", "libmp3lame"], {"mp3"}),
            ("audio.m4a", ["-vn", "-c:a", "aac"], {"aac"}),
            ("audio.aac", ["-vn", "-c:a", "aac"], {"aac"}),
            ("audio.ogg", ["-vn", "-c:a", "libvorbis"], {"vorbis"}),
            ("audio.flac", ["-vn", "-c:a", "flac"], {"flac"}),
            ("audio.wav", ["-vn", "-c:a", "pcm_s16le"], {"pcm_s16le"}),
            ("animated.gif", ["-an", "-vf", "fps=8,scale=128:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse", "-loop", "0"], {"gif"}),
        ]
        result = {"status": "PASS", "scope": "11 application output formats and GIF, encode/probe/full decode; does not establish Nova GUI acceptance", "cases": []}
        for name, options, expected in cases:
            self.runtime_run("ffmpeg", common + ["-i", "odd-input.mp4", *options, name], cwd=smoke)
            info = json.loads(self.runtime_run("ffprobe", ["-v", "error", "-show_streams", "-show_format", "-of", "json", name], cwd=smoke))
            actual = {stream["codec_name"] for stream in info["streams"]}
            if actual != expected or float(info["format"]["duration"]) < 0.5:
                raise ValueError(f"Runtime smoke case has unexpected codecs/duration: {name} {actual}")
            self.runtime_run("ffmpeg", ["-nostdin", "-hide_banner", "-loglevel", "error", "-i", name, "-f", "null", "-"], cwd=smoke)
            result["cases"].append({"file": name, "codecs": sorted(actual), "bytes": (smoke / name).stat().st_size, "full_decode": "PASS"})
            print(f"PASS runtime encode/probe/decode: {name}", flush=True)
        return result

    def package(self) -> dict:
        self.assert_recipe_unchanged()
        (self.out / "bin").mkdir(parents=True)
        suffix = ".exe" if self.is_windows else ""
        binary_records = []
        versions = {}
        for name in ("ffmpeg", "ffprobe"):
            binary = self.out / "bin" / (name + suffix)
            shutil.copy2(self.build("ffmpeg") / (name + suffix), binary)
            binary.chmod(0o755)
            if self.target == "macos-arm64":
                self.run("sign", ["codesign", "--force", "--sign", "-", "--timestamp=none", str(binary)], self.work)
                self.run("sign", ["codesign", "--verify", "--strict", str(binary)], self.work)
            binary_bytes = binary.read_bytes()
            if any(form.encode("utf-8") in binary_bytes for original, _token in self.replacements
                   for form in {original, original.replace("\\", "/"), original.replace("\\", "\\\\")}):
                raise ValueError(f"Private build/source/home path remains embedded in {binary.name}")
            arch = architecture(binary, self.target)
            deps = self.dependencies(binary)
            version = self.runtime_run(name, ["-hide_banner", "-version"])
            if f"version {self.entries['ffmpeg']['version']}" not in version.splitlines()[0]:
                raise ValueError(f"Wrong FFmpeg version in {name}")
            if str(Path.home()) in version or "--enable-nonfree" in version or "--enable-version3" not in version:
                raise ValueError("Runtime configuration contains a personal home path or unexpected licensing flags")
            versions[name] = version
            binary_records.append({"path": binary.relative_to(self.out).as_posix(), "sha256": sha256(binary), "bytes": binary.stat().st_size, "architecture": arch, "dependencies": deps})
        encoders = self.runtime_run("ffmpeg", ["-hide_banner", "-encoders"])
        available = set(re.findall(r"^\s*[VAS][A-Z.]{5}\s+(\S+)", encoders, re.M))
        if REQUIRED_ENCODERS - available:
            raise ValueError(f"Missing required encoders: {sorted(REQUIRED_ENCODERS - available)}")
        filters = self.runtime_run("ffmpeg", ["-hide_banner", "-filters"])
        filter_names = set(re.findall(r"^\s*[TSC.]{3}\s+(\S+)", filters, re.M))
        if REQUIRED_FILTERS - filter_names:
            raise ValueError(f"Missing required filters: {sorted(REQUIRED_FILTERS - filter_names)}")
        decoders = self.runtime_run("ffmpeg", ["-hide_banner", "-decoders"])
        if not re.search(r"^\s*V[A-Z.]{5}\s+libdav1d\s", decoders, re.M):
            raise ValueError("Pinned libdav1d AV1 decoder is unavailable")
        protocols = self.runtime_run("ffmpeg", ["-hide_banner", "-protocols"])
        if re.search(r"^\s*(?:https?|tcp|udp|tls|rtmp\w*|srt|ftp)\s*$", protocols, re.M):
            raise ValueError("Network protocol unexpectedly enabled")
        if self.target == "macos-arm64" and "h264_videotoolbox" not in available:
            raise ValueError("macOS VideoToolbox encoder exposure regressed")
        smoke = self.smoke()
        self.collect_compiler_runtime_notices()
        self.copy_corresponding_source()
        self.write_provenance({"versions": versions, "encoders": encoders, "filters": filters, "decoders": decoders, "protocols": protocols})
        write_json(self.out / "build-manifest.json", {"schema_version": 1, "platform": self.target, "source_set_id": self.lock["source_set_id"],
                   "sources_lock_sha256": hashlib.sha256(self.recipe_files["sources.lock.json"]).hexdigest(),
                   "builder_sha256": hashlib.sha256(self.recipe_files["build_runtime.py"]).hexdigest(), "patches": [],
                   "commands": self.commands, "file_operations": self.operations, "tools": self.tools,
                   "environment": {key: self.public(self.env[key]) for key in CONTROLLED_ENV if key in self.env},
                   "compiler_runtime": self.compiler_runtime, "static_prefix": file_manifest(self.prefix),
                   "binary_files": binary_records, "native_runtime_smoke": smoke})
        manifest = {"schema_version": 1, "provider": "Nova source build", "provider_url": "https://ffmpeg.org/", "source_set_id": self.lock["source_set_id"],
                    "platform": self.target, "architecture": "arm64" if self.target == "macos-arm64" else "x86_64",
                    "ffmpeg_version": self.entries["ffmpeg"]["version"], "license": "GPL-3.0-or-later", "patches": [],
                    "distribution_status": "source-complete-native-runtime-validation", "source_status": "Original verified source archives, license texts, recipe, tool versions and exact normalized build commands included. Application/native GUI/release acceptance is separate.",
                    "native_runtime_check": "PASS", "required_encoders": sorted(REQUIRED_ENCODERS), "required_filters": sorted(REQUIRED_FILTERS),
                    "native_runtime_smoke": smoke, "binary_files": binary_records, "sources": self.lock["sources"],
                    "compiler_runtime_policy": "Only OS dynamic libraries are permitted. GCC compiler support libraries may be linked statically under the GCC Runtime Library Exception; MinGW-w64 runtime is a toolchain component. See NOTICE.md and provenance/tool-versions.json.",
                    "files": file_manifest(self.out, ("manifest.json",))}
        write_json(self.out / "manifest.json", manifest)
        return manifest

    def assert_recipe_unchanged(self) -> None:
        for name, initial in self.recipe_files.items():
            path = self.lock_path if name == "sources.lock.json" else HERE / name
            if path.read_bytes() != initial:
                raise ValueError(f"Build recipe changed while compilation was running: {name}. Preserve logs and rebuild with a frozen recipe")

    def collect_compiler_runtime_notices(self) -> None:
        """Preserve installed support-code notices, never just web links.

        GCC's support files are toolchain runtime exceptions, not additional
        media libraries. Record all candidate static support files, a
        conservative superset; this is not a claim that every object was used.
        """
        if self.target == "macos-arm64":
            self.compiler_runtime = {"policy": "Apple system toolchain/runtime; no redistributed GCC/MinGW support libraries", "status": "system-runtime"}
            return
        names = ["libgcc.a", "libgcc_eh.a", "libstdc++.a", "libssp.a", "libssp_nonshared.a"]
        if self.is_windows:
            names += ["libmingw32.a", "libmingwex.a", "libwinpthread.a", "crt2.o", "crtbegin.o", "crtend.o"]
        archives = []
        packages: dict[str, dict] = {}
        licenses: dict[str, Path] = {}
        for name in names:
            output = self.run("toolchain-notices", [self.cc, "-print-file-name=" + name], self.work, capture=True).strip()
            path = Path(output)
            if output == name or not path.is_file():
                if name in ("libgcc.a", "libmingw32.a", "libmingwex.a", "crt2.o"):
                    raise ValueError(f"Cannot locate expected compiler support file: {name}")
                continue
            path = path.resolve()
            if self.is_windows:
                owner = self.run("toolchain-notices", ["pacman", "-Qqo", self.posix(path)], self.work, capture=True).strip()
            elif shutil.which("dpkg-query"):
                owner_output = self.run("toolchain-notices", ["dpkg-query", "-S", str(path)], self.work, capture=True)
                owner = owner_output.splitlines()[0].split(": ", 1)[0]
            else:
                owner = self.run("toolchain-notices", ["rpm", "-qf", str(path)], self.work, capture=True).strip()
            if not owner or "\n" in owner:
                raise ValueError(f"Ambiguous compiler support package owner: {name}")
            archives.append({"name": name, "path": self.public(str(path)), "sha256": sha256(path), "bytes": path.stat().st_size, "owner": owner})
            if owner in packages:
                continue
            if self.is_windows:
                version = self.run("toolchain-notices", ["pacman", "-Q", owner], self.work, capture=True).strip()
                listing = self.run("toolchain-notices", ["pacman", "-Qlq", owner], self.work, capture=True).splitlines()
                candidates = []
                for filename in listing:
                    if "/share/licenses/" in filename and not filename.endswith("/"):
                        native = subprocess.run(["cygpath", "-w", filename], check=True, capture_output=True, text=True).stdout.strip()
                        if Path(native).is_file():
                            candidates.append((filename.lstrip("/"), Path(native)))
            elif shutil.which("dpkg-query"):
                version = self.run("toolchain-notices", ["dpkg-query", "-W", "-f=${Package} ${Version}", owner], self.work, capture=True).strip()
                base = owner.split(":", 1)[0]
                candidate = Path("/usr/share/doc") / base / "copyright"
                candidates = [(f"{base}/copyright", candidate)] if candidate.is_file() else []
            else:
                version = owner
                listing = self.run("toolchain-notices", ["rpm", "-ql", owner], self.work, capture=True).splitlines()
                candidates = [(filename.lstrip("/"), Path(filename)) for filename in listing
                              if re.search(r"(?:license|copying|copyright|exception)", filename, re.I) and Path(filename).is_file()]
            if not candidates:
                raise ValueError(f"No installed copyright/license bytes found for compiler support package {owner}")
            packages[owner] = {"version": version, "license_files": []}
            for relative, source in candidates:
                key = owner.replace(":", "_") + "/" + relative
                licenses[key] = source
                packages[owner]["license_files"].append("licenses/compiler-runtime/" + key)
        # Debian copyright files reference these complete texts by path.
        for name in ("GPL-2", "GPL-3", "LGPL-2", "LGPL-2.1", "LGPL-3"):
            source = Path("/usr/share/common-licenses") / name
            if not self.is_windows and source.is_file():
                licenses["common-licenses/" + name] = source
        text = "\n".join(path.read_text(encoding="utf-8", errors="replace") for path in licenses.values())
        if "GCC RUNTIME LIBRARY EXCEPTION" not in text.upper() or not re.search(r"Version\s+3\.1", text, re.I):
            raise ValueError("Installed compiler notices do not include the full GCC Runtime Library Exception 3.1")
        for relative, source in licenses.items():
            output = self.out / "licenses/compiler-runtime" / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, output)
        self.compiler_runtime = {"status": "PASS", "scope": "Conservative candidate static compiler support files; package notices preserved, not an object-level link-use claim", "support_files": archives, "packages": packages}

    def copy_corresponding_source(self) -> None:
        source_out = self.out / "source"
        (source_out / "archives").mkdir(parents=True)
        for name in ORDER:
            entry = self.entries[name]
            shutil.copyfile(self.cache / "archives" / entry["archive"], source_out / "archives" / entry["archive"])
            for item in entry["license"]["file_hashes"]:
                relative = PurePosixPath(item["path"])
                if relative.is_absolute() or ".." in relative.parts:
                    raise ValueError("Unsafe license path in source lock")
                source = self.source(name).joinpath(*relative.parts)
                if source.stat().st_size != item["bytes"] or sha256(source) != item["sha256"]:
                    raise ValueError(f"Source license differs from lock: {name}/{relative}")
                target = self.out / "licenses" / name / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
        for name, content in self.recipe_files.items():
            (source_out / name).write_bytes(content)
        # Acquisition manifests contain local cache paths on some hosts, so
        # describe the retained bytes directly using relative portable paths.
        write_json(source_out / "source-manifest.json", {"schema_version": 1, "source_set_id": self.lock["source_set_id"], "patches": [],
                   "archives": [{"path": "archives/" + e["archive"], "sha256": e["sha256"], "bytes": e["archive_bytes"]} for e in self.lock["sources"]]})
        notice = """Nova FFmpeg runtime — source build

FFmpeg and ffprobe are configured with GPL and version 3 enabled, without
nonfree components. The combined executable is GPL-3.0-or-later. The Nova
application has its own license. Individual dependency terms and copyright
notices are retained under licenses/ and in the complete original archives.

source/archives/ contains the exact nine upstream source archives used by
this build, with no applied patches. source/sources.lock.json records their
URLs, hashes and revisions. source/BUILD.md and source/build_runtime.py are
the build and installation instructions. build-manifest.json records the
native toolchain versions and exact commands; {WORK}, {SOURCES}, {RUNTIME}
and {RECIPE} identify replaceable local directories. Build logs published in
provenance/ have those local paths normalized. Unnormalized build logs stay
in the builder's private work directory.

All non-system media dependencies are linked statically. Dynamic imports
are restricted to the target operating system's libraries/frameworks.
Windows requires a supported Windows installation with UCRT; it does not
require MSYS2 at run time. Linux uses the native runner's glibc baseline;
no older-distribution compatibility is promised by this source recipe.
macOS uses system frameworks including VideoToolbox and libSystem.

Build tools are not bundled. GCC compiler runtime/support code, if emitted
or linked into these executables, is covered by the GCC Runtime Library
Exception 3.1: https://www.gnu.org/licenses/gcc-exception-3.1.html . This
exception permits independent combinations without extending GPL source
requirements to the compiler itself. MinGW-w64 runtime/header code has its
own permissive licenses: https://www.mingw-w64.org/license/ . Native compiler
versions are recorded in provenance/tool-versions.json. Installed GCC/MinGW
license/copyright/exception bytes are copied under licenses/compiler-runtime/
on Linux and Windows. These toolchain
exceptions are distinct from the nine linked media-library source archives.

Runtime encode/probe/decode checks do not establish application GUI acceptance,
hardware encoder execution, notarization, or release approval. Consult the
separate native application validation report for those results.
"""
        (self.out / "NOTICE.md").write_text(notice, encoding="utf-8")

    def write_provenance(self, capabilities: dict[str, str]) -> None:
        directory = self.out / "provenance"
        (directory / "logs").mkdir(parents=True)
        write_json(directory / "commands.json", self.commands)
        write_json(directory / "tool-versions.json", self.tools)
        write_json(directory / "capabilities.json", capabilities)
        for log in self.raw_logs.glob("*.log"):
            (directory / "logs" / log.name).write_text(self.public(log.read_text(encoding="utf-8", errors="replace")), encoding="utf-8")
        # Configure diagnostics identify static linkage decisions without
        # retaining object files or personal build-directory paths.
        for name in ORDER:
            for parent in (self.build(name), self.source(name)):
                for relative in ("config.log", "config.mak", "ffbuild/config.log", "ffbuild/config.mak", "meson-logs/meson-log.txt", "CMakeCache.txt"):
                    source = parent / relative
                    if source.is_file():
                        output = directory / "configuration" / name / relative
                        output.parent.mkdir(parents=True, exist_ok=True)
                        output.write_text(self.public(source.read_text(encoding="utf-8", errors="replace")), encoding="utf-8")

    def execute(self) -> dict:
        self.preflight()
        self.prepare()
        self.recipes()
        if self.args.dry_run:
            return {"status": "NOT_RUN", "platform": self.target, "source_set_id": self.lock["source_set_id"],
                    "commands": self.commands, "file_operations": self.operations, "tools": self.tools,
                    "note": "Verified archives and planned commands only; nothing compiled or packaged"}
        return self.package()


def self_test() -> None:
    import io
    import unittest

    class BuildContractTests(unittest.TestCase):
        def test_mac_vpx_target_selects_desktop_sdk(self):
            # Upstream dispatches arm*-darwin-* to iOS and versioned
            # *-darwin2[0-5]-* to the desktop macOS SDK.
            self.assertRegex(VPX_TARGETS["macos-arm64"], r"^arm64-darwin2[0-5]-gcc$")

        def test_vpx_public_prefix_resolves_from_both_build_directories(self):
            self.assertFalse(PurePosixPath(VPX_INSTALL_PREFIX).is_absolute())
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp).resolve()
                for library in ("libvpx", "ffmpeg"):
                    self.assertEqual((root / "build" / library / VPX_INSTALL_PREFIX).resolve(), root / "prefix")

        def test_dynamic_library_boundary(self):
            for target, allowed, denied in [
                ("macos-arm64", "/System/Library/Frameworks/VideoToolbox.framework/Versions/A/VideoToolbox", "@rpath/libx264.dylib"),
                ("linux-x86_64", "libc.so.6", "libx264.so.165"),
                ("windows-x86_64", "api-ms-win-crt-runtime-l1-1-0.dll", "libwinpthread-1.dll"),
            ]:
                self.assertTrue(allowed_dependency(allowed, target))
                self.assertFalse(allowed_dependency(denied, target))
            self.assertFalse(allowed_dependency("libgcc_s_seh-1.dll", "windows-x86_64"))
            self.assertFalse(allowed_dependency("libstdc++.so.6", "linux-x86_64"))

        def test_archive_rejects_escape_links_and_wrong_root(self):
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                for index, name in enumerate(("pkg/../escape", "/pkg/escape", "other/file")):
                    archive = root / f"bad-{index}.tar"
                    with tarfile.open(archive, "w") as writer:
                        item = tarfile.TarInfo(name)
                        item.size = 1
                        writer.addfile(item, io.BytesIO(b"x"))
                    with self.assertRaises(ValueError):
                        extract_source(archive, root / f"out-{index}", "pkg")
                archive = root / "link.tar"
                with tarfile.open(archive, "w") as writer:
                    item = tarfile.TarInfo("pkg/link")
                    item.type = tarfile.SYMTYPE
                    item.linkname = "../../escape"
                    writer.addfile(item)
                with self.assertRaises(ValueError):
                    extract_source(archive, root / "out-link", "pkg")

        def test_archive_exact_bytes_modes(self):
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                archive = root / "good.tar"
                with tarfile.open(archive, "w") as writer:
                    item = tarfile.TarInfo("pkg/configure")
                    item.size = 4
                    item.mode = 0o755
                    writer.addfile(item, io.BytesIO(b"test"))
                extract_source(archive, root / "src", "pkg")
                self.assertEqual((root / "src/configure").read_bytes(), b"test")
                if os.name != "nt":
                    self.assertTrue((root / "src/configure").stat().st_mode & 0o111)

        def test_normalized_commands_keep_arguments_separate(self):
            builder = object.__new__(Builder)
            builder.replacements = [("/Users/someone/work", "{WORK}"), ("/Users/someone", "{HOME}")]
            self.assertEqual(builder.public("-I/Users/someone/work/prefix/include"), "-I{WORK}/prefix/include")
            self.assertEqual(builder.public("/Users/someone/cache"), "{HOME}/cache")

    suite = unittest.defaultTestLoader.loadTestsFromTestCase(BuildContractTests)
    if not unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful():
        raise SystemExit(1)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=PLATFORMS)
    parser.add_argument("--sources", type=Path, help="Acquisition cache root containing archives/")
    parser.add_argument("--work-dir", type=Path, help="New or empty private build directory")
    parser.add_argument("--destination", type=Path, help="New runtime directory; never overwritten")
    parser.add_argument("--lock", type=Path, default=HERE / "sources.lock.json")
    parser.add_argument("--jobs", type=int, default=min(os.cpu_count() or 1, 8))
    parser.add_argument("--dry-run", action="store_true", help="Verify sources/tools and print normalized build plan; do not compile")
    parser.add_argument("--self-test", action="store_true", help="Run build-driver contract tests without compiling")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if any(getattr(args, name) is None for name in ("platform", "sources", "work_dir", "destination")):
        parser.error("--platform, --sources, --work-dir and --destination are required")
    if args.jobs < 1 or args.jobs > 128:
        parser.error("--jobs must be between 1 and 128")
    result = Builder(args).execute()
    if args.dry_run:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(json.dumps({"runtime": str(args.destination.resolve()), "platform": args.platform, "native_runtime_check": result["native_runtime_check"], "smoke_cases": len(result["native_runtime_smoke"]["cases"]), "source_archives": len(result["sources"])}, indent=2))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError, tarfile.TarError) as error:
        print(f"Runtime build failed: {error}", file=sys.stderr)
        raise SystemExit(1)
