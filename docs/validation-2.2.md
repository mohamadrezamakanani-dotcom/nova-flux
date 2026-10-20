# Historical validation: Nova Flux 2.2

This record describes a previous local release. Paths and screenshots named here were local delivery evidence and are not included in the public source checkout. Current status is in `../VALIDATION.md`.

# Nova Flux 2.2 validation

Validated locally on Apple Silicon macOS. This report separates automated engine tests, native UI acceptance and distribution checks. The old web preview and 2.1 heuristic/resolution screenshots do not describe this release.

## Corrections

- Quality numbers are obtained from real, cancellable sample encodes using the production profile. Short sources are measured whole; longer re-encodes use three timeline positions. Stream copy uses a complete, cancellable private remux without seek/trim or extrapolation, under the same 15-second deadline. Missing or failed measurement shows Measuring or Unavailable, never an invented numerical fallback. Queue totals require every queued file.
- Sample size extrapolation remains approximate. Sample spread is observed evidence, not a guaranteed interval; unsampled scenes and encoder fallback can change full-file size. Final output bytes are measured.
- Target size is a per-file maximum shown once in its own section, with exact bytes. It is not a quality/resolution prediction. Successful outputs must be within the byte cap; otherwise no output is published.
- Video dimensions and cadence are preserved in ordinary and target modes. No scale/padding/frame-rate-reduction filter is used. Old saved resize settings are reset to Original. Infeasible caps fail clearly. Odd dimensions use compatible software encoders; unsupported AVI geometry fails instead of changing dimensions.
- Queued sources are revalidated in a cancellable background worker, before conversion and before publication. Replaced/mutated sources invalidate measurements. Source files remain untouched. Each published filename is new and atomic.
- A per-user OS file lock prevents duplicate native GUI instances across app copies and activates the existing instance. Crash-stale file contents do not retain the OS lock.

## Automated verification

**85 tests passed, zero failed or ignored:** 22 library, 13 desktop/single-instance helpers, 3 input/queue boundary, 6 engine, 19 measured-estimate/source-refresh, 7 performance, 7 geometry/cadence and 8 target-size tests. The final full run includes long-GOP copy accuracy and incomplete-remux rejection for video and audio. **All-target Clippy passed with warnings denied**, and formatting passed. Results are recorded in docs/evidence. Real-media tests use the bundled FFmpeg/ffprobe, not mocks. Coverage includes all eleven output formats and three encoding qualities, compatible/incompatible stream copy, audio precision, original geometry/cadence, strict byte limits, cancellation/cleanup, concurrent publication, source mutation, measured estimate behavior and calibration fallback.

Reproduce from the source archive:

```sh
NOVA_TEST_RUNTIME="$PWD/runtime" cargo test --locked -- --include-ignored --test-threads=1
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
python3 scripts/package.py --runtime runtime --destination 'Nova Flux.app'
```

## Native macOS acceptance

The installed 2.2.0 window imported a generated **8,987,163-byte, 1920×1080, 30 fps** source. Quality cards finished measuring and displayed distinct estimates: Compact approximately 806.5 KB, Balanced 2.68 MB, High 3.75 MB, Stream copy 8.99 MB. The output panel opened/closed correctly without blocking measurement.

Starting a real conversion through the native window with a **1,000,000-byte maximum** produced **967,193 bytes**, a measured **89.2% reduction**, through Apple VideoToolbox after local CPU/hardware comparison. Independent probing and full decoding confirmed **1920×1080, 30 fps, 90 decoded frames, 3.000-second video, 3.008-second audio/container duration**. The source SHA-256 stayed unchanged. The byte cap was not presented as a quality estimate. The destination was a private test folder; user preferences were backed up and restored.

Launching a second installed executable while the native queue contained one file exited successfully without an activation error. The primary PID and queue remained; only one GUI process existed. The app subsequently quit and reopened correctly. The legacy saved 480p setting loaded as Original, and the user's output folder and 80 MB size preference were restored. The exact user source was reimported for visible numeric verification; its complete 40-minute timeline was not re-encoded as part of this UI test.

The optimized release build, bundled-runtime diagnostic, deep/strict ad-hoc signature verification and all 38 runtime manifest byte counts/hashes passed. Seven known installed app copies were updated to the same 2.2.0 binary; previous copies were kept as recoverable non-app backups. Original Nova Converter.app's asar hash remained unchanged.

## Numeric source check

The source shown in the user's screenshot was independently inspected on this Mac. Its current filesystem size and ffprobe size agree at **266,873,590 bytes = 266.873590 MB = approximately 254.51 MiB**. The actual source is 1920×1080 at 25 fps, with a 2425.742222-second container duration. The previously reported 289 MB does not match this exact local file. The final engine measured a complete private stream-copy remux at **265,850,175 bytes** in approximately **0.51 seconds**, without seeking, trimming or extrapolation. The smaller copy omits unselected artwork. This measurement preserves the selected full timeline and is removed afterwards. The final UI is checked against that measurement. Numbers are not adjusted to match a verbal size. Only primary non-artwork video and the first audio stream are selected.

## Practical limits

The Rust UI/queue/planning/process control use native egui/eframe/wgpu; bundled FFmpeg/ffprobe and their native libraries perform encoding. There is no HTML, JavaScript, WebView, upload, cloud account or runtime download. Automatic scheduling compares the implemented CPU and Apple VideoToolbox paths and budgets parallel work against CPU and available memory. It does not prove a mathematical speed optimum on all future devices. Measuring estimates and first-use calibration have a real cost and are cancelled before conversion starts.

Complete-copy measurements need temporary disk space. A real low-disk test returned Unavailable with the underlying storage error, and succeeded after removing this task's disposable generated debug-build cache; user files, source, release binary and installed app were retained.

This local build is arm64 and ad-hoc signed. Intel, other operating systems and Apple notarization have not been validated. Public distribution still requires the appropriate signing/notarization process and review of the bundled GPL/runtime license notices. Tests cannot guarantee every damaged, DRM-protected or unusual media file. Arbitrary non-quarter-turn rotations that cannot be safely preserved are rejected for re-encoding; compatible stream copy can retain them. AVI timestamp precision depends on its representable clock; unreliable AVI header average-frame-rate/count values are not asserted as decoded facts. Queue history is session-only. Subtitle and alternate-audio preservation are outside the current selected-stream contract.
