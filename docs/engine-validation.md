> Historical initial-engine milestone. Current release results and superseding geometry/estimate behavior are in ../VALIDATION.md.

# Baseline Rust media engine validation

This records the earlier12-test baseline. The final expanded suite, adaptive-performance checks and native Rust GUI/package evidence are in the root VALIDATION.md.

Validated on this Mac with the bundled native arm64 FFmpeg/ffprobe runtime, 18 October 2026. Tests use the same public `Engine` API as the desktop adapter. This document covers the engine; native UI and packaged-app acceptance are separate checks.

## Passing test suite

Command, from the source directory (set the runtime directory to the packaged or staging runtime that contains `bin/ffmpeg`, `bin/ffprobe`, and `lib/`):

```sh
NOVA_TEST_RUNTIME=/absolute/path/to/runtime cargo test --no-default-features -- --include-ignored --test-threads=1
```

The completed run passed **12 tests**: 3 unit tests, 3 boundary integration tests, and 6 engine integration tests. No test failures or compiler warnings remained.

- **33 encode/probe/full-decode combinations:** MP4, MKV, AVI, MOV, WebM, MP3, WAV, AAC, OGG, M4A, and FLAC at Low, Medium, and High. Each output was independently probed and fully decoded with FFmpeg `-xerror`; the selected stream types matched the output category. These tests used a generated H.264/AAC fixture with a Persian filename containing spaces and shell metacharacters. Source bytes remained unchanged.
- **WAV precision:** dedicated source/output fixtures verified 24-bit and 32-bit integer PCM and 32-bit and 64-bit floating-point PCM. The 24-bit output's `bits_per_raw_sample` was independently checked as 24. There was no dedicated 16-bit source precision regression in this run.
- **Unequal stream lengths:** 5-second video with 1.2-second audio, and 1.2-second video with 5-second audio, each converted to MP3 and MP4. Extraction retained the selected audio duration and video-container output retained the longer selected stream. These four conversions passed independent duration checks.
- **Original mode:** compatible H.264/AAC MP4 remux succeeded. Incompatible H.264/AAC to WebM copy was rejected per file; retrying that job with MP4 succeeded. The entire Original-mode container/codec compatibility matrix has not been exercised live.
- **Invalid inputs and routing:** corrupt media remained visible as an error job; WMA audio converted to MP3; audio-only input to a video target and silent video to an audio target were rejected. FLAC was discovered correctly in nested folders.
- **Queue/lifecycle:** repeated and overlapping imports deduplicated canonical paths; recursive symbolic links were skipped; cancelling one waiting item preserved neighbouring conversions and the configured worker bound. Cancelling an active batch removed temporary files, published no result, and preserved source files. Clearing completed items retained failed and cancelled items for retry.
- **Validation/output safety:** invalid format, quality, concurrency, and missing output folders were rejected. Atomic publication selected a fresh name when an existing destination was present, preserving the existing bytes and handling Unicode output names.

## Corrections verified during development

The initial live matrix exposed an invalid high WebM mono Opus bitrate; the high preset now uses 192 kb/s. Dedicated regressions first reproduced and then verified fixes for WAV precision loss, incorrectly comparing extracted audio with a longer video-container duration, and clearing failed/cancelled jobs.

## Scope and practical limits

- Media decoding/encoding remains FFmpeg; the application engine and process/queue control are Rust. No shell is used to launch converters. Inputs are constrained to local `file,pipe` protocols.
- The engine converts the first non-artwork video stream and first audio stream. Additional audio tracks, subtitles, data streams, and artwork are not preserved. Common metadata is copied when supported by the destination.
- Import inspection and output verification use bounded ffprobe subprocesses. Successful production jobs are probed before atomic publication; the exhaustive decode pass described above is a test check, not a decode performed after every user conversion.
- Duration comparison uses matching selected streams only when their durations are known. Unknown durations cannot establish completeness; progress can remain at zero until completion when a useful duration is unavailable.
- WAV transcoding preserves the tested source PCM precision. FLAC output is integer PCM and is not claimed to preserve arbitrary floating-point samples exactly. Compressed presets can change bit depth, colour format, or audio channels for compatibility; MP3/AVI/WebM downmix inputs with more than two channels to stereo.
- Folder discovery is bounded to 5,000 media files, 100,000 entries, 1,000 directories, and 10 levels. Symbolic links are skipped. Unsupported explicitly selected files are visible errors; non-media files encountered inside folders are skipped with a summary.
- The tested fixtures are small generated media, not every possible input, codec, damaged file, filesystem, or long-running workload. Dedicated cover-art, odd-dimension, exhaustive remux, and blocked-probe timeout fixtures were not part of this completed suite.
- Runtime and engine validation here are arm64-only. Intel/universal operation is not established.
