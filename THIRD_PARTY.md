# Third-party and asset notices

The MIT license at the repository root applies to Nova Flux's own application source, tests, packaging scripts and project-created graphics. Third-party projects retain their licenses.

## Rust dependencies

Cargo.lock records exact resolved dependency versions. Direct dependencies include serde/serde_json, uuid, tempfile, egui/eframe/wgpu through eframe, rfd and image. Their source/license notices are supplied by their respective crates. Dependencies and egui's bundled default fonts can carry MIT, Apache, BSD and font-specific notices; a public compiled binary needs the applicable complete notices. A Cargo.lock entry is not a replacement for those notices. This source publication does not include copied crate source trees or a compiled application binary.

## Codec runtime

FFmpeg, ffprobe and codec libraries are separate executables/libraries with their own licenses. The supplied local macOS runtime and pinned private Windows/Linux validation runtimes enable GPL components. They are not covered or relicensed by Nova Flux's MIT license.

The runtime build/source provenance scripts and [portable-build notes](scripts/PORTABLE_BUILD.md) identify providers, pinned revisions, hashes and known limits. Complete matching corresponding-source/build material has not been verified for the prebuilt runtime closures. Therefore third-party media binaries are excluded from this public source repository and public release assets. Private CI artifacts remain private. Public self-contained packages require a separately verified source-and-notice distribution.

See https://ffmpeg.org/legal.html for the FFmpeg project's own guidance.

## Project icon

The navy/coral loop icon in assets/ was created for this project using local procedural drawing. It is separate from the supplied Nova Converter application's original icon. No original app asset or private media file is included in this source release. Platform font files are read from the user's existing operating system when available; they are not copied into this repository.
