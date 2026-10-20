# Build the media runtime from retained source

This recipe builds FFmpeg and ffprobe natively for macOS arm64, Linux x86_64,
or Windows x86_64. It uses the nine exact archives in `sources.lock.json`,
builds the eight external media libraries statically into a private prefix,
and enables GPL version 3 in FFmpeg. No source patches are applied. It
does not download an existing binary runtime.

The output includes both executables, the complete original source archives,
all locked license texts, this recipe and driver, source hashes, toolchain
versions, exact build commands and configuration logs. Local private paths
in public logs/manifests use `{WORK}`, `{SOURCES}`, `{RUNTIME}` and `{RECIPE}`
placeholders. The original full build logs remain in the private build
directory. Source archives are copied byte for byte, not repacked.

## Native tools

Use Python 3.10 or later. Required tools are a native C/C++ compiler,
`ar`, `ranlib`, `nm`, `strip`, Bash, GNU Make, pkg-config/pkgconf, CMake,
Meson and Ninja. x86_64 builds additionally require NASM. The builder records
the installed tool versions; it does not install tools or activate services.
Compiler and operating-system versions define the tested compatibility
baseline. This recipe does not promise binary-identical output across
different compilers, SDKs or OS releases.
Choose build, cache and output directory paths without whitespace or shell/
Make metacharacters; the upstream configure/Make recipes require that boundary.

- **macOS arm64:** Xcode Command Line Tools and native arm64 CMake, Meson,
  Ninja and pkgconf. Homebrew packages may supply build tools; Homebrew
  media libraries must not be linked. The deployment target is macOS 11.0,
  subject to the SDK and separately validated application requirements.
  libvpx uses `arm64-darwin20-gcc`, its desktop macOS target. Its similarly
  named unversioned `arm64-darwin-gcc` target selects iOS and is not used.
- **Linux x86_64:** GCC/G++, GNU binutils and the tools above. For Debian or
  Ubuntu, the corresponding packages are `build-essential`, `cmake`,
  `meson`, `ninja-build`, `pkg-config`, `nasm`, `python3` and `xz-utils`.
  The final media executables use the build host's system glibc. Validate on
  the oldest distribution you intend to support. Installed GCC support-file
  notices are resolved through `dpkg-query` (Debian/Ubuntu) or `rpm`; the
  owning packages must retain their copyright/license files.
- **Windows x86_64:** use the **MSYS2 UCRT64** shell and native UCRT64
  Python/GCC. MSYS2 package names are `make`, `diffutils`, and
  `mingw-w64-ucrt-x86_64-{gcc,python,cmake,meson,ninja,pkgconf,nasm}`
  (install each expanded package name). The builder requires
  `MSYSTEM=UCRT64`, native Windows Python and GCC from `/ucrt64/bin`.
  Native Python paths are converted for Bash configure scripts using
  `cygpath`; CMake/Meson receive native paths. MSYS2 is a build dependency
  and must not be required by the finished binaries.

## Acquire, build, verify

From the Nova repository root:

```sh
python3 scripts/runtime-source/acquire_sources.py --cache-dir ../runtime-source-cache --extract
python3 scripts/runtime-source/build_runtime.py --self-test
python3 scripts/runtime-source/build_runtime.py \
  --platform macos-arm64 \
  --sources ../runtime-source-cache \
  --work-dir ../runtime-build-macos-arm64 \
  --destination ../runtime-macos-arm64 \
  --jobs 8
```

Use `--platform linux-x86_64` or `--platform windows-x86_64` on those native
hosts and choose separate build/destination directories. In UCRT64 use
`python` if that is the native interpreter's command. Build and output
directories must be new or empty; existing artifacts are never overwritten.
The cache may already contain verified archives, so an offline build needs
no acquisition step. The builder independently checks every original
archive's byte length and SHA-256 before extracting a private build copy.
It ignores modified acquisition-cache source trees. A nonempty patch list
is rejected until the driver is explicitly extended to verify and apply it.

`--dry-run` checks the host, tools and archive hashes, then prints the full
normalized command plan without extracting, compiling or packaging. It is
planning evidence only.

The driver automatically verifies architecture, dynamic dependencies,
license/configuration flags, CPU encoders, required filters, AV1 decoder
exposure, and absence of network protocols. It then generates, probes and
fully decodes all eleven Nova output formats: MP4, MOV, MKV, WebM, AVI,
MP3, M4A, AAC, Ogg Vorbis, FLAC and WAV. An additional GIF palette workflow
checks built-in filters. Odd source dimensions exercise padding for video
encoders. Smoke media stay in the private work directory.

These media-runtime checks are separate from the Nova application engine,
GUI, real hardware encoding, packaging, signing/notarization and three-OS
release acceptance. A successful builder run does not establish those.

## Controlled dependency boundary

FFmpeg retains its built-in codec, muxer, demuxer, protocol and filter defaults,
except networking is disabled. Autodetection is disabled. Exactly x264,
LAME, Ogg/Vorbis, Opus, libvpx, dav1d and zlib are built from this source
set. No other external media libraries may be discovered through pkg-config.
The pkg-config search directories point only to the private prefix.
Static archives must exist for all eight external libraries, and installing
any DLL, dylib, shared object or DLL import archive into that prefix fails
the build.

macOS explicitly enables the system VideoToolbox framework and permits
only `/System/Library/` and `/usr/lib/` dynamic imports. Linux permits only
the standard glibc/system ELF imports listed in the driver, rejects runtime
search paths, and statically links GCC support libraries. Windows uses
`-static -static-libgcc -static-libstdc++`; only Windows system DLLs/API-set
contracts may be imported. Imports such as `msys-2.0.dll`,
`libwinpthread-1.dll`, `libgcc_s_seh-1.dll` and media-library DLLs are rejected.
No dependent DLL is silently copied from the build host.

zlib 1.3.2 names its MinGW static archive `libzs.a` while its pkg-config file
and FFmpeg's check request `-lz`. On Windows the driver makes an identical
installed `libz.a` alias and records that operation in the build manifest.
Vorbis 1.3.7's original CMake files are compatible with CMake 4 through
the explicit `CMAKE_POLICY_VERSION_MINIMUM=3.5` configure option. Neither
operation edits upstream source.

Build flags avoid machine-specific x86 tuning. Runtime CPU dispatch remains
available. File/debug prefix maps remove private source paths from compiled
`__FILE__` strings; FFmpeg's reported configure options use a generic
installation prefix and relative include/library paths. The output is checked
for private build/source/home paths both in binary bytes and in FFmpeg's
reported configuration.
libvpx also embeds its own configure arguments in the executable. Its
installation prefix is therefore `../../prefix` relative to
`{WORK}/build/libvpx`; the installed pkg-config paths resolve to the same
private prefix when invoked from the sibling `{WORK}/build/ffmpeg` directory.
This prevents the build directory from becoming part of libvpx's public
configuration string without altering upstream sources or binary bytes.

## Rebuild a distributed runtime

After unpacking a Nova distribution, `runtime/source/archives/` is already
a complete acquisition cache. Run its adjacent `build_runtime.py` with
`--sources runtime/source`, a new private `--work-dir`, and a new
`--destination`. The default lock lives beside the driver. Install the
native build tools listed above and use the recorded compiler/tool versions
when reproducing a particular artifact. `runtime/build-manifest.json`
contains the native build commands and explicit file alias operation;
`runtime/provenance/` contains sanitized configuration diagnostics.

The eight media dependencies and FFmpeg have their own license texts under
`runtime/licenses/`. The source archive retains all additional copyright
headers and upstream files. Compiler/system runtime terms are distinct:
the GCC Runtime Library Exception and MinGW-w64 permissive runtime licenses
are referenced in `runtime/NOTICE.md`. Linux and Windows additionally retain
the installed compiler support packages' actual license/copyright/exception
bytes under `runtime/licenses/compiler-runtime/`; missing notices fail the
build. A conservative support-file inventory records package versions and
hashes without claiming that every candidate object was linked. Compiler
binaries are not bundled.
Public release still requires the separate source/license audit and native
application acceptance required by the release workflow.

## Upstream build references

- [FFmpeg platform guidance](https://ffmpeg.org/platform.html) and
  [license/source guidance](https://ffmpeg.org/legal.html).
- [x264's configure script at the locked commit](https://code.videolan.org/videolan/x264/-/blob/b35605ace3ddf7c1a5d67a2eb553f034aef41d55/configure).
- [libvpx's native/out-of-tree build documentation](https://chromium.googlesource.com/webm/libvpx/+/v1.17.0/README).
- [dav1d's build documentation](https://code.videolan.org/videolan/dav1d/-/blob/1.5.4/README.md)
  and [build options](https://code.videolan.org/videolan/dav1d/-/blob/1.5.4/meson_options.txt).
- [zlib 1.3.2 CMake targets](https://github.com/madler/zlib/blob/v1.3.2/CMakeLists.txt),
  [Ogg CMake options](https://github.com/xiph/ogg/blob/v1.3.6/CMakeLists.txt),
  [Vorbis CMake options](https://github.com/xiph/vorbis/blob/v1.3.7/CMakeLists.txt),
  and [Opus CMake options](https://github.com/xiph/opus/blob/v1.6.1/CMakeLists.txt).
- [MSYS2 UCRT64 environment](https://www.msys2.org/docs/environments/)
  and [filesystem path handling](https://www.msys2.org/docs/filesystem-paths/).
