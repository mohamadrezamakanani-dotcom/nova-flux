# Nova Flux 2.3 validation

Evidence date: 2026-10-20. A passing check applies to its stated host and scope.

| Check | Host | Status | Evidence |
| --- | --- | --- | --- |
| Full Rust suite, including real codec acceptance tests | Local Apple Silicon macOS | PASS: 100 tests, 0 failures | All-target locked tests with ignored acceptance tests included, single test thread |
| All-target Clippy, warnings denied | Local Apple Silicon macOS | PASS | Locked dependencies, current source |
| Python extraction, packaging and validation boundaries | Local macOS | PASS: 17 tests | Unittest discovery under scripts |
| Native Windows build, codec tests and rendered GUI | Windows Server 2022 x86_64 CI | IN PROGRESS | Independent native validation run |
| Native Linux build, codec tests and rendered GUI | Ubuntu 22.04 x86_64 CI | IN PROGRESS | Independent native validation run |
| Optimized 2.3 macOS application and native acceptance | Local Apple Silicon macOS | IN PROGRESS | Separate from source tests |

## What the Mac tests verify

The Rust suite comprises 31 library tests, 13 desktop tests, 3 file-boundary tests, 6 engine tests, 19 estimate tests, 7 performance tests, 3 portable-platform tests, 9 geometry tests and 9 target-size tests. Real codec acceptance tests are explicitly enabled; a skipped acceptance test is not counted as passing.

The suite exercises every offered output format, actual source-relative video compression, exact explicit limits, cancellation/cleanup, immutable inputs, output collision protection, remux duration and full-file size measurement, and production/calibration profile parity. Geometry tests cover odd dimensions, high frame rates, variable timestamps, display aspect ratio and supported rotation. Unsupported geometry fails without silently resizing.

Two generated four-second fixtures demonstrate real preset reductions. One efficient 183,680-byte input produced 70,268 / 119,228 / 151,384 bytes for Compact / Balanced / High quality, below the respective 73,472 / 119,392 / 156,128-byte maxima. A 1,800,795-byte fixture produced 708,015 / 1,123,390 / 1,475,247 bytes, below 720,318 / 1,170,516 / 1,530,675. These retain 640×360, 25 fps and 100 frames. They are examples, not universal compression ratios or perceptual-quality guarantees. A minimal source fails clearly when the requested maximum cannot fit.

An explicit 200,000-byte lossy-audio limit produced a valid 163,048-byte MP3 even though the source itself was smaller than the requested maximum. Explicit limits therefore remain the user's exact limit and are not accidentally replaced by preset percentages.

## Limits and distribution

Video quality cards show verified source-relative maxima, not exact final-size predictions. Lossy compression can reduce both video and audio fidelity. Completed outputs show actual bytes. Original geometry, frame cadence and selected duration remain fixed.

Native CI rendering is narrower than manual interaction on a physical desktop. Windows/Linux GPU encoding, every desktop dialog, every distribution/driver and all processor architectures are not established by these checks. See [platform scope](docs/CROSS_PLATFORM.md).

The public repository contains MIT application source and no FFmpeg runtime binaries. Private CI packages are not public releases. Complete corresponding runtime/dependency sources remain outstanding before public self-contained application packages can be distributed. The local Mac app is ad-hoc signed, not Apple-notarized.

[Historical 2.2 local validation](docs/validation-2.2.md) is retained separately.
