# Nova Flux

A local media converter with a native English interface written in Rust. Convert media on your own device, choose a verified size maximum, and keep original video dimensions and frame cadence.

![Native Nova Flux interface](assets/screenshots/macos.png)

The UI, queue, settings, compression planning, resource scheduling and process control use Rust with egui/eframe/wgpu. FFmpeg and ffprobe are separate native codec tools. The application has no WebView, JavaScript UI, uploads, cloud account or runtime download.

## Platform status

Rust supports many targets, but each operating system and processor requires its own executable, media runtime, packaging and tests. One macOS application cannot be used as a Windows or Linux executable.

| Platform | Target | Status |
| --- | --- | --- |
| macOS / Apple Silicon | arm64 | 2.3 tested locally: 100 tests, native Metal window and actual UI conversion |
| Windows | x86_64 | 2.3 native CI: 78 tests, DX12/WARP rendering and packaged launch |
| Linux | x86_64 | 2.3 native CI: 100 tests, X11/Vulkan rendering and packaged launch |

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

Use Rust 1.99 or newer. Windows needs the MSVC build tools; Linux needs its desktop/window-system development libraries and a working Vulkan-capable WGPU adapter. The [native validation workflow](.github/workflows/portable-validation.yml) records concrete dependencies and commands.

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

Build-time acquisition and private portable packaging are documented in [PORTABLE_BUILD.md](scripts/PORTABLE_BUILD.md). Public runtime redistribution is deliberately separate from source publication: matching runtime/dependency sources and notices have not yet been verified as complete. Do not attach the private CI runtime packages to a public release. The local macOS build is ad-hoc signed and has not been Apple-notarized.

## License

Nova Flux application source is [MIT licensed](LICENSE). Third-party dependencies, system fonts and FFmpeg keep their own licenses; MIT does not relicense them. See [third-party and asset notices](THIRD_PARTY.md).
