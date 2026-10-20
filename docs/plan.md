# Nova Flux implementation plan

- [x] Inventory supplied app and preserve original.
- [x] Rust engine with eleven output targets, safe outputs and queue lifecycle.
- [x] Native FFmpeg/ffprobe dependency closure and new icon.
- [x] Strict size-budget planning, measured retries and acceptance tests.
- [x] Automatic device/workload calibration, CPU/RAM budgets and fallback.
- [x] Engine, native UI and real-media checks established; current results are recorded in VALIDATION.md.
- [x] Replace prototype WebView with native Rust egui interface.
- [x] Compile/check native UI and review background work/budget validation.
- [x] Exercise final app, slider/number/convert/cancel and preferences on this Mac.
- [x] Package app and source; publish exact evidence and practical limits.

## 2.2 correctness release

- [x] Replace heuristic quality numbers with measured, cancellable local encodes.
- [x] Separate the per-file size ceiling from estimates; display exact measured bytes.
- [x] Detect changed queued sources and revalidate before publication.
- [x] Lock original video dimensions/cadence in ordinary and target-size paths.
- [x] Prevent duplicate GUI instances across installed application copies.
- [x] Run final all-target real-media tests, lint, release build and package checks.
- [x] Verify installed native UI, real compression, preferences and duplicate activation.
