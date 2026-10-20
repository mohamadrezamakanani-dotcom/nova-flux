> Historical development notes through version 2.2. Local delivery paths and runtime archives mentioned below are not public source assets. See [current validation](../VALIDATION.md) and [platform status](CROSS_PLATFORM.md).

# Nova Flux design

Latest scope: a Rust-native, English-only replacement for the supplied Nova Converter, with a new icon/UI, every original conversion target, target-size compression and measured automatic performance selection. Earlier Tauri/bilingual prototypes are superseded.

## Application

The interface, application state, settings, queue, import/probing orchestration, size planning, calibration, concurrency and process lifecycle are Rust. The GUI uses egui/eframe and wgpu. FFmpeg/ffprobe remain bundled native codec tools; operating-system APIs and codec internals are not claimed to be Rust. No HTML, JavaScript, WebView, Electron, uploads or runtime downloads are part of the delivered application.

Preserve the five video targets MP4/MKV/AVI/MOV/WebM and five audio targets MP3/WAV/AAC/OGG/M4A; add FLAC. Native file/folder selection, drag/drop, queue filters, metadata, cancel/retry/remove/reveal, completed-item clearing and persisted preferences use the public Rust engine API. GUI work must stay responsive while dialogs/import/start/shutdown run in workers. Minimum window920x650, default1180x800, scrollable settings, visible conversion footer. Palette: navy/ivory/coral with a geometric loop icon.

## Media and output

Convert the first non-artwork video and first audio stream. Originals and existing destinations are preserved. Local file/pipe protocols only; argv subprocesses without shells. Probe and duration checks precede atomic no-clobber publication. Temporary files, calibration samples and pass logs are owned and removed on failure/cancellation. Copy mode checks codec/container compatibility and never silently re-encodes. WAV preserves tested PCM precision; FLAC is integer audio compression.

## Size budget

Optional logarithmic mouse slider plus exact numeric decimal MB (1MB=1,000,000bytes), presets and per-output limits. Slider maximum follows the smallest active input; invalid values remain visible and disable Start. CPU video uses two passes; compatible hardware encoding uses actual-size feedback with software fallback. Budget planning allocates stream bitrates and adjusts encoding quality/audio settings. Original display dimensions, pixel aspect/orientation and frame cadence are invariants; no resizing, padding or frame-rate reduction is allowed. Legacy resize requests are rejected by the engine and saved UI resize preferences reset to Original. Successful output never exceeds the requested byte cap. Do not pad bytes, truncate the final timeline or drop required streams to fake compliance. Unknown duration, lossless/copy combinations and impossible budgets return clear errors.

## Measured size previews

Use real production-profile local encodes, cancellable off the drawing thread. Display Measuring/Unavailable until evidence exists, aggregate only complete queue measurements and distinguish sample extrapolation from guaranteed limits. Revalidate queued source revisions and exact bytes; invalidate stale measurements on replacement and recheck before publication.

## Automatic performance

Actual short CPU/Apple-hardware samples compare implemented paths on the device and workload. Trials are cancellable, private and bounded to six seconds per new class. Seven-day cache keys include device/OS/app/runtime/workload properties and reject malformed, expired or future entries. Reassess CPU/RAM headroom each batch; schedule workers/threads without artificial memory allocation. CPU mode remains selectable. No claim of a universal mathematical optimum across all future algorithms or files.

## Validation and delivery

Core live matrix includes all eleven targets at three qualities, full decode, stream/duration contracts, corrupt media, Unicode names, precision, collision safety, traversal bounds, cancellation and concurrency. Size and adaptive tests cover strict caps, every lossy target, portrait display shape, Matroska duration tags, real hardware/failure fallback, cache invalidation and calibration cancellation. Final GUI and package checks are separate, recorded in VALIDATION.md. Deliver a locally signed Apple Silicon app, source/runtime archive and validation report.
