# Build and validate Nova Flux

## Build and validate

Release builds use Rust **1.99.0** and locked Cargo dependencies. CI uses Python **3.12**; release helpers require Python 3.11 or newer. Windows needs the MSVC build tools; Linux needs its desktop/window-system development libraries and a working WGPU adapter. The [matched-runtime workflow](../.github/workflows/matched-runtime.yml) records concrete dependencies and commands.

This source repository **does not contain the third-party media runtime binaries**. Put compatible FFmpeg/ffprobe under `runtime/bin/` for a Windows/Linux portable package, or pass an absolute runtime directory through `NOVA_RUNTIME` for development. Windows uses `ffmpeg.exe` and `ffprobe.exe`. A macOS `.app` uses `Contents/Resources/runtime/`.

```sh
cargo test --locked --no-default-features
cargo build --release --locked
NOVA_RUNTIME=/absolute/path/to/runtime target/release/nova-flux --diagnose
```

For a Windows development build in PowerShell (with compatible tools already in `runtime/bin/`):

```powershell
$env:NOVA_RUNTIME = (Resolve-Path .\runtime).Path
cargo build --release --locked
.\target\release\nova-flux.exe --diagnose
.\target\release\nova-flux.exe
```

For real media tests, set `NOVA_TEST_RUNTIME` to an absolute compatible runtime directory and run:

```sh
cargo test --locked -- --include-ignored --test-threads=1
cargo clippy --all-targets --locked -- -D warnings
python -B -m unittest scripts/test_portable_packaging.py -v
```

The application supports an isolated native rendering test with `--smoke-test`, `NOVA_DATA_DIR`, `NOVA_CACHE_DIR` and `NOVA_SMOKE_SCREENSHOT`. That test creates a native WGPU window, receives an actual rendered screenshot and closes gracefully. It is narrower than manual user interaction on physical computers.

## Matched codec sources and release packages

Version 2.3.1 replaces the older provider runtimes with our own native FFmpeg **8.1.3** builds. Nine exact original archives cover FFmpeg, x264, LAME, libogg, libvorbis, Opus, libvpx, dav1d and zlib. Hashes, original license texts, build instructions, flags and tool versions are retained. External library autodetection and media networking are disabled; required codec libraries are linked statically, with native system imports inspected. No nonfree components are enabled.

Each verified application package contains the complete codec source archives and recipe under its runtime directory, the application MIT license, exact Rust/font notices and Rust standard-library notices. The separate `Nova-Flux-2.3.1-FFmpeg-sources.zip` is published alongside corresponding binaries; its hash and download URL are embedded in each package. See [source preparation](../scripts/runtime-source/SOURCE_BUNDLE.md), [native runtime build](../scripts/runtime-source/BUILD.md), [notice collection](RUST_NOTICES.md), and [verified packaging](VERIFIED_PACKAGING.md).

To rebuild the media tools, follow the exact native recipe. Windows uses **MSYS2 UCRT64** GCC, Make, **Perl**, diffutils, CMake, Meson, Ninja, pkgconf and NASM; Rust application builds use the separate native MSVC environment. Install Rust 1.99.0 `rustfmt`, `clippy` and **`rust-docs`** for release notice collection. Windows distribution builds use `-C target-feature=+crt-static`. The collector and packager reject incomplete or stale source/notice inventories.

The [2.3.1 validation record](../VALIDATION.md) identifies actual native test/build revisions and package acceptance. Older provider-based private packages and the [2.3.0 source preview](https://github.com/mohamadrezamakanani-dotcom/nova-flux/releases/tag/v2.3.0) are historical evidence. They are not the matched 2.3.1 distributions. macOS packages are ad-hoc signed and **not Apple-notarized**; Windows packages are **not code-signed**. These checks do not certify legal compliance, visual quality or compatibility with every device.


## Public repeatable release checks

The [public native build guide](PUBLIC_RELEASE_BUILD.md) describes the manual workflow that builds matched source runtimes, collects notices, tests and extracts packages on each native target. It creates test artifacts; it does not automatically publish or replace releases.
