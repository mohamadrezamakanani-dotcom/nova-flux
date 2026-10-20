# Changelog

## Unreleased

- Clear application downloads, installation and troubleshooting guides.
- Community reporting forms, contribution/security policies and platform acceptance checklist.
- Public repeatable native source/runtime/package validation and compact hash-bound evidence.
- Repository discovery metadata, shared-link artwork and genuine conversion demonstration.

This file records shipped changes and their evidence. Planned work belongs in
the [roadmap](docs/ROADMAP.md).

## 2.3.1 — 2026-10-21

- Published native packages for macOS arm64, Windows x86_64, and Linux x86_64
  using project-built FFmpeg 8.1.3/ffprobe and eight statically linked external
  libraries from nine exact original source archives. Media networking and
  external library autodetection are disabled; no nonfree components are enabled.
- Included retained original codec sources, build instructions, license texts,
  target-specific Rust/font/toolchain notices, and package inventories. Published
  the matching FFmpeg source ZIP alongside the binary assets, with its URL and
  hash bound into each package.
- Verified package extraction, inventory integrity, bundled diagnosis, and actual
  native framebuffer rendering on all three targets. Native Rust/media suites
  passed **100 macOS / 78 Windows / 100 Linux** tests with no failures or ignored
  tests on the recorded hosts. Windows excludes Unix-only instrumentation.
- Manually checked the installed macOS app's native picker, a 500,000-byte
  maximum conversion producing 486,199 bytes, preserved 1920×1080/30 fps/90 frames,
  and license navigation. Physical Windows/Linux manual acceptance remains
  **NOT_RUN**. CI software graphics adapters do not establish physical GPU support.

This release retains the 2.3 conversion behavior, including source-relative video
maxima, explicit byte limits, original video geometry/cadence, and selected-stream
handling. macOS is ad-hoc signed and not notarized; Windows is unsigned. See the
[2.3.1 validation record](VALIDATION.md) and
[release assets](https://github.com/mohamadrezamakanani-dotcom/nova-flux/releases/tag/v2.3.1).

## 2.3.0 — historical source preview

The [2.3.0 public release](https://github.com/mohamadrezamakanani-dotcom/nova-flux/releases/tag/v2.3.0)
was a source preview. Validation covered source-relative 40/65/85% video maxima,
explicit byte limits, original geometry/cadence, local media estimates, queue
controls, and native platform builds. Provider-runtime packages and related native
CI results were private validation evidence, not the matched 2.3.1 public binary
distributions. See the [historical validation](docs/validation-2.3.0.md); its
outstanding source/distribution limits describe that historical release.
