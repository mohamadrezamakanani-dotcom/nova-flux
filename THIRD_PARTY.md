# Third-party and asset notices

The MIT license at the repository root applies to Nova Flux's own application source, tests, packaging scripts and project-created graphics. Third-party projects retain their licenses.

## Rust dependencies

Cargo.lock records exact dependency versions. Compiled packages retain target-selected original license, copyright and NOTICE files under `licenses/rust/`, including egui's embedded font terms and Rust toolchain/standard-library inventories. The collector verifies original crate archive checksums and exact-commit upstream supplements; its small upstream notice cache is retained in this repository for offline collection. A Cargo.lock entry alone is not a replacement for notices. See [Rust and font notices](docs/RUST_NOTICES.md) for the selected terms and explicitly documented upstream notice-text omissions.

## Codec runtime

FFmpeg, ffprobe and codec libraries retain their own licenses. The source-build recipe enables GPL components and version 3, without nonfree components. The combined FFmpeg/ffprobe executables are distributed under GPL-3.0-or-later. They are not covered or relicensed by Nova Flux's MIT license.

The [source lock and native build recipe](scripts/runtime-source/BUILD.md) select nine exact original archives. Source-built runtime packages retain all nine archives byte for byte, component license texts, the build driver, configuration, commands, compiler versions and a file hash inventory under `runtime/`. The same original media sources are also supplied as a separate release ZIP. [Verified packaging](docs/VERIFIED_PACKAGING.md) binds each package to that ZIP, its own native runtime and the matching Rust/font notices. **About & licenses** provides access from the application.

Older local/provider runtimes used for 2.3.0 validation have incomplete matching source provenance. They remain excluded from public binary releases; the legacy private packager gates are unchanged. Provider provenance is documented in the historical [portable-build notes](scripts/PORTABLE_BUILD.md). Source-build packaging uses a separate verified path.

See https://ffmpeg.org/legal.html for the FFmpeg project's own guidance.

## Project icon

The navy/coral loop icon in assets/ was created for this project using local procedural drawing. It is separate from the supplied Nova Converter application's original icon. No original app asset or private media file is included in this source release. Platform font files are read from the user's existing operating system when available; they are not copied into this repository.

## Repository introduction media

The social-preview banner was generated with ImageGen using the project-created icon as an identity reference; it redraws that reference and does not replace the application icon. [Banner provenance](assets/social-preview-provenance.md) records the prompt, processing and hashes.

The [demonstration](docs/DEMO_PROVENANCE.md) uses an actual native application screenshot and generated acceptance media with the verified app output. HyperFrames rendered the composition; its font shapes appear only as rendered pixels, not redistributed font files. It contains no personal media, fabricated application interaction, third-party endorsement or encoding-speed claim. These project-created introduction assets are covered by the application repository's MIT terms; referenced third-party projects keep their own licenses.
