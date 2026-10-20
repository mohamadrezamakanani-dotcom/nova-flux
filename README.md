# Nova Flux

A local media converter with a native English interface written in Rust. Convert media on your own device, choose a verified size maximum, and keep original video dimensions and frame cadence.

![Native Nova Flux interface](assets/screenshots/macos.png)

The UI, queue, settings, compression planning, resource scheduling and process control use Rust with egui/eframe/wgpu. FFmpeg and ffprobe are separate native codec tools. The application has no WebView, JavaScript UI, uploads, cloud account or runtime download.

## Platform status

Rust supports many targets, but each operating system and processor requires its own executable, media runtime, packaging and tests. One macOS application cannot be used as a Windows or Linux executable.

| Platform | Target | Status |
| --- | --- | --- |
| macOS / Apple Silicon | arm64 | 2.3.1: 100 tests, native Metal window and actual UI conversion |
| Windows | x86_64 | 2.3.1: 78 tests, DX12/WARP rendering and extracted-package launch |
| Linux | x86_64 | 2.3.1: 100 tests, X11/Vulkan rendering and extracted-package launch |

This does not promise support for every Linux distribution, Windows version, processor, physical GPU, Android or iOS. See [cross-platform validation](docs/CROSS_PLATFORM.md) for actual OS/runner evidence. Versioned [validation records](VALIDATION.md) distinguish local acceptance, native CI and untested behavior.

## Conversion

Add files, drop them onto the window, or add a folder. Choose an output format and destination, then start the queue. Cancel individual items or the batch, retry a failed item, reveal an output in the file manager, or clear completed items. **Output settings** opens and closes the **Your output** panel.

Video output: MP4, MKV, AVI, MOV and WebM. Audio output: MP3, WAV, AAC, OGG, M4A and FLAC. The selected streams are the first non-artwork video and first audio stream; subtitles, artwork and alternate audio are outside this selected-stream contract. Broken files report their own errors. Folder scanning is bounded and does not follow symbolic links. Originals and existing output files are not overwritten. Closing the app cancels active work; queue history is session-only.

## Source-aware compression

For video, the quality cards choose distinct **maximum sizes relative to each source file**:

| Preset | Maximum output size |
| --- | --- |
| Compact | 40% of source bytes |
| Balanced | 65% of source bytes |
| High quality | 85% of source bytes |

These percentages are deliberate size goals, not perceptual-quality scores or fabricated predictions. The interface labels them as maximums. Smaller presets can reduce visible quality and audio fidelity; retaining dimensions does not retain original quality. CPU encoding uses two-pass bitrate allocation; compatible hardware encoding uses measured feedback and software fallback. The completed file is measured before publication and must fit the applied maximum. Exact output bytes and reduction are displayed afterwards. A very small or already minimal source may not fit a requested preset; that fails clearly and publishes no oversized or resized file.

**Video dimensions are locked to Original.** Compression does not resize, pad, lower frame rate or shorten the selected streams. Orientation and pixel aspect ratio are verified where known. Unsupported geometry/rotation combinations fail clearly rather than silently changing the image. Old saved resize settings reset to Original.

**Stream copy** remuxes compatible selected streams without re-encoding. Its size is measured through one complete private copy, with no seek, trim or extrapolation. It requires temporary disk space and has a cancellable 15-second measurement deadline. A failed measurement shows Unavailable with the reason. Container compatibility still applies.

Audio size estimates use real local encodes with the production profile. Longer re-encodes sample three timeline positions; short sources are measured whole. Sample extrapolation is approximate, and the tooltip records the measurement method. Missing measurements never become invented numbers or partial batch totals. WAV preserves decoded PCM precision; FLAC uses integer lossless compression and cannot recover quality already lost in a compressed source. These lossless formats can be larger than the input.

Input sizes use decimal MB: **1 MB = 1,000,000 bytes**. Exact source/output bytes are also displayed, and MiB is distinguished in the input tooltip. Changed queued sources invalidate their metadata and measurements; sources are checked again before conversion and publication.

## Exact size maximum

Enable **Target file size** to drag the logarithmic slider or enter a per-output maximum in decimal MB. This explicit maximum overrides the source-relative presets. Quality is calculated automatically; the same cap is not repeated as a pretend prediction under every quality option.

A successful output is at or below its maximum. The app does not pad it to an exact byte count. Original dimensions, cadence and full selected duration remain fixed. If the codec cannot fit, no output is saved. WAV, FLAC and Stream copy do not offer a lossy target-size mode.

## Performance

Automatic mode budgets worker count and CPU threads against the workload, processor count and available memory. On supported Apple encoders it compares real short CPU and hardware trials, caches the measured choice and falls back to software where needed. Other platforms currently use compatible software encoding; no untested universal GPU acceleration is claimed. RAM is used for media work, rather than allocated merely to increase its usage number.

Exact size control has a cost: CPU video uses two passes. Automatic tuning compares implemented paths; it is not proof of the fastest possible algorithm on every future device. Measurements and source probes are cancellable and stay away from the drawing thread.

## Build and validate

Release builds use Rust **1.99.0** and locked Cargo dependencies. CI uses Python **3.12**; release helpers require Python 3.11 or newer. Windows needs the MSVC build tools; Linux needs its desktop/window-system development libraries and a working WGPU adapter. The [matched-runtime workflow](.github/workflows/matched-runtime.yml) records concrete dependencies and commands.

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

Each verified application package contains the complete codec source archives and recipe under its runtime directory, the application MIT license, exact Rust/font notices and Rust standard-library notices. The separate `Nova-Flux-2.3.1-FFmpeg-sources.zip` is published alongside corresponding binaries; its hash and download URL are embedded in each package. See [source preparation](scripts/runtime-source/SOURCE_BUNDLE.md), [native runtime build](scripts/runtime-source/BUILD.md), [notice collection](docs/RUST_NOTICES.md), and [verified packaging](docs/VERIFIED_PACKAGING.md).

To rebuild the media tools, follow the exact native recipe. Windows uses **MSYS2 UCRT64** GCC, Make, **Perl**, diffutils, CMake, Meson, Ninja, pkgconf and NASM; Rust application builds use the separate native MSVC environment. Install Rust 1.99.0 `rustfmt`, `clippy` and **`rust-docs`** for release notice collection. Windows distribution builds use `-C target-feature=+crt-static`. The collector and packager reject incomplete or stale source/notice inventories.

The [2.3.1 validation record](VALIDATION.md) identifies actual native test/build revisions and package acceptance. Older provider-based private packages and the [2.3.0 source preview](https://github.com/mohamadrezamakanani-dotcom/nova-flux/releases/tag/v2.3.0) are historical evidence. They are not the matched 2.3.1 distributions. macOS packages are ad-hoc signed and **not Apple-notarized**; Windows packages are **not code-signed**. These checks do not certify legal compliance, visual quality or compatibility with every device.

## License

Nova Flux application source is [MIT licensed](LICENSE). Third-party dependencies, system fonts and FFmpeg keep their own licenses; MIT does not relicense them. See [third-party and asset notices](THIRD_PARTY.md).
