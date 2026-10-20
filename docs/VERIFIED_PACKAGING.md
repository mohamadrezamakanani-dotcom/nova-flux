# Verified native binary packaging

`scripts/package_verified.py` assembles the public source-backed distribution on
the target operating system. This path is separate from the older private-CI
packagers and does not change their public-distribution gates. Python 3.11+ is
required. It does not build, download, publish, or replace existing artifacts.

```sh
python3 scripts/package_verified.py \
  --platform macos-arm64 \
  --runtime ../nova-runtime-macos \
  --notices ../nova-notices-macos \
  --binary target/release/nova-flux \
  --source-bundle ../Nova-Flux-2.3.1-FFmpeg-sources.zip \
  --destination ../nova-release-macos
```

Use `windows-x86_64` or `linux-x86_64` on its native host, supplying that target's
binary, runtime, and notices. Windows packaging can run from native Python in
PowerShell; MSYS2's `objdump.exe` is located on PATH or under
`C:/msys64/ucrt64/bin/`, or the runner's temporary MSYS2 installation. Set
`NOVA_OBJDUMP` to an explicit existing tool path for another installation.
macOS needs `otool` and `codesign`; Linux needs `readelf`.
The destination must not exist. A failed package is not published and an
existing release is never overwritten; select a fresh destination for a retry.

## Required inputs and gates

The runtime must be the current `Nova source build` schema-1 output from
`scripts/runtime-source/build_runtime.py`. The packager verifies every listed
payload file and rejects missing, extra, changed, duplicate, unsafe, or linked
files. It checks native platform/architecture, FFmpeg/ffprobe binary hashes,
recorded system-only imports, successful runtime checks, and agreement between
runtime and build manifests. Linux/Windows builds must retain actual GCC/MinGW
compiler-support license files identified by their installed owner packages.

The runtime must contain all nine exact locked original source archives,
matching dependency license texts, source manifest, and the exact current
acquisition driver, builder, lock, and build instructions. Runtime builder/lock
hashes are checked against those actual retained bytes and the current recipe.

The separate source ZIP is independently verified through
`package_sources.verify_zip`. Its source-set ID, original archive inventory,
source lock and builder hashes must match the runtime's retained corresponding
source and current recipe. Its ZIP SHA-256 and a release download URL are bound
into the public package manifest and README. The complete codec sources are
also already inside the binary distribution's `runtime/source/`, so it does
not depend on a URL-only source offer. The ZIP itself is a separate release
asset and is not redundantly copied into the binary archive.

Rust/font notices must match the current application name/version, exact
`Cargo.lock` SHA-256, native target triple, and Rust 1.99.0 toolchain. Every
listed crate and toolchain notice and the Cargo dependency tree are verified;
unknown/unlisted files are rejected. See `RUST_NOTICES.md` for the collector's
license choices, embedded fonts, retained upstream cache, and reviewed original
notice-text omissions.

The application's own architecture and actual native imports are inspected.
macOS imports must resolve to system framework/library paths. Linux imports
must belong to the explicit operating-system/desktop list and have no embedded
RPATH/RUNPATH. Windows imports must be approved Windows DLLs; external MSVC
redistributable DLLs are rejected. Build the Windows Rust executable using
`-C target-feature=+crt-static` for the distribution's standalone CRT contract.
Dynamically loaded optional OS/GPU facilities still need native application
acceptance testing.

The privacy gate checks staged bytes for this machine's actual home and
repository paths. It rejects leaks in executable/source/notice bytes and requires
a rebuild with source paths remapped. Generated build logs alone receive path
normalization during staging. FFmpeg's configure diagnostic dumps the entire
shell environment; its public copy is replaced with an omission notice. Exact
commands, flags, tools, source hashes and final config.mak remain available.
The runtime manifest records original and packaged log hashes and the original
runtime-manifest hash. Executables, build commands, original source archives,
recipes, copyrights and license text are unchanged. Unmodified diagnostics stay
in private CI evidence; they are unnecessary for building the media executables.

## Package layout and native verification

Output folder and archive root:
`Nova-Flux-<version>-<platform>/`.

macOS contains `Nova Flux.app` with English metadata, build number 7, minimum
macOS 12, the project icon, `Contents/MacOS/nova-flux`, and
`Contents/Resources/runtime/`. It creates only the outer app signature; it does
not resign or modify the already signed media executables. A fresh runtime hash
verification immediately after signing enforces this. License navigation is at
`Contents/Resources/licenses/README.txt`; original Rust/font/toolchain notices
are under `licenses/rust/`, and the application MIT text is
`licenses/application-LICENSE.txt`.

Windows/Linux keep `nova-flux.exe`/`nova-flux`, `runtime/`, `licenses/`, icon and
README together. The license directory has the same internal structure as the
macOS app. Runtime dependency notices and complete codec source remain inside
`runtime/`.

The staged executable runs `--diagnose` with `NOVA_RUNTIME` and dynamic-library
overrides removed. It must report `ready=true`, no error, and the current
compiled application version. `package-manifest.json` records the actual app
imports, source ZIP URL/hash, runtime/notices manifest hashes, diagnosis and
an exact inventory of all package files except the manifest itself.
`application_executable`, `runtime_directory`, and `license_directory` are paths
relative to the archive root, so independent verifiers need not guess layout.

The packager then writes a ZIP on macOS/Windows or a tar.gz on Linux, extracts
it into a separate temporary directory using safe bounded extraction, verifies
every extracted file, rechecks runtime/notices, and runs the extracted
application's diagnosis again. macOS signatures are verified after extraction.
Archive entries preserve executable permission bits and do not store the local
account's owner/group names. The archive SHA-256 is written beside the final
archive only after these checks pass.

Standard output is JSON with `archive`, `sha256`, `package_directory`,
`version`, `platform`, `status`, and `scope`. A failed gate returns nonzero.
The configured source release URL is
`https://github.com/mohamadrezamakanani-dotcom/nova-flux/releases/download/v<version>/<source-zip-name>`;
publish that exact source asset alongside the corresponding binary archives.

Successful packaging establishes the recorded input, staging, extraction,
source/notice integrity, and native diagnosis checks. GUI rendering, actual
conversion workflows, compatibility on other operating-system versions,
Apple notarization, and publication are separate acceptance steps.

Focused tests:

```sh
python3 -m unittest discover -s scripts -p test_verified_packaging.py
```
