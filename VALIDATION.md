# Nova Flux 2.3.1 validation

Evidence date: 2026-10-21 (Europe/Rome). A passing result applies only to its
stated host, revision and scope. [Machine-readable receipt](docs/evidence/matched-2.3.1.json).

| Check | macOS arm64 | Windows x86_64 | Linux x86_64 |
| --- | --- | --- | --- |
| Locked Rust tests including real media acceptance | PASS: 100 | PASS: 78 | PASS: 100 |
| Failures / ignored tests | 0 / 0 | 0 / 0 | 0 / 0 |
| Formatting, all-target Clippy with warnings denied | PASS | PASS | PASS |
| Optimized app, bundled diagnosis | PASS | PASS | PASS |
| Own FFmpeg build: encode/probe/full decode smoke | PASS: 12 cases | PASS: 12 cases | PASS: 12 cases |
| Source/runtime/notices/import integrity | PASS | PASS | PASS |
| Extracted package diagnosis and actual native rendering | PASS | PASS | PASS |
| Manual native picker, conversion and license navigation | PASS | NOT_RUN | NOT_RUN |

Native environments: macOS 27.0.1 / Apple M4 / Metal; Windows Server 2022 /
DX12 Microsoft Basic Render Driver (WARP); Ubuntu 22.04 / X11-Xvfb / Vulkan
llvmpipe (Mesa 23.2.1). Software CI adapters are not physical GPU acceptance.

Application/media source acceptance used `25529569e1c6d6d24c6979d50fe7e6f97601b046`
for Mac/Linux and `83dc3c6f798b1e030c56f781986ed9cd9fe4f74b` for Windows.
The latter changes build integration; application code and codec recipe are
identical. Native CI application runs: Linux 39470592443/job118793314030;
Windows 39472802531/job118800957336. Their application tests passed separately
from original packaging attempts. Final Windows/Linux packaging and extracted
render acceptance passed in run **39513080541** on packaging revision `31b6ab2`.
The exact accepted executable bytes were reused, not rebuilt under a new claim.

## Real conversions and preserved video

The full suite exercises all eleven offered output formats, source-relative
40/65/85% video maxima, explicit hard byte limits, impossible limits, source
immutability, output collisions, cancellation and cleanup. Geometry checks cover
odd dimensions, high rates, variable timestamps, display aspect and supported
rotation. Unsupported combinations fail without resizing. Selected streams are
the first non-artwork video and first audio; alternate audio/subtitles are outside
this contract. Windows excludes Unix-only process instrumentation, explaining its
different actual count. No skipped acceptance test is counted as passing.

In the installed 2.3.1 Mac app, a generated **3,653,251-byte** fixture was imported
through the native picker. Entering **0.5 MB (500,000 bytes)** and clicking Start
produced **486,199 bytes**, confirmed both in the app and independently. Probing
and complete decoding retained **1920×1080, 30 fps, all 90 frames**. The container
reported 3.008 seconds; the input's 3.021-second container includes audio padding.
CPU encoding was selected after real CPU/Apple calibration (267/281 ms). Source
bytes remained unchanged. Only the generated test item was cleared; original user
preferences were restored, and the original Nova Converter stayed untouched.
The About & licenses dialog and Finder navigation to bundled notices were
visibly checked. Each of the three current local app copies passed inventory,
signature and bundled diagnosis checks.

Quality cards show verified source-relative maximums, not invented final-size
predictions. A successful output may be smaller than its cap; tiny impossible
limits fail without saving an oversized/resized/shortened result. Keeping video
geometry does not preserve original visual quality. The earlier 2.3.0 preset
examples remain in the [historical record](docs/validation-2.3.0.md).

## Matching sources, notices and packages

All three runtimes retain the exact same nine original archives: FFmpeg8.1.3,
x264, LAME, libogg, libvorbis, Opus, libvpx, dav1d and zlib. No source patches or
nonfree components are enabled. The builder SHA-256 is
`52d7698ae4ca415b7515bf8bf030f5ac005a9726b6c1a553334400f4ecd9df4b`.
The shared source ZIP SHA-256 is
`2d455cac22950eabcadbd23f8e30a147d887516403c51821b1af766d80ca05f5`.
It is included as a separate release asset; exact original archives and build
instructions also remain inside every application package's runtime.

The native codec builds audit actual system imports and retain GCC/MinGW support
notices where applicable. Target-specific Rust/font/standard-library notices are
collected against Rust1.99.0 and the exact Cargo.lock. Packaging verifies original
license/source hashes without rewriting upstream notices. Generated build logs
alone undergo documented privacy processing: FFmpeg's complete shell-environment
dump stays private; exact build commands/configuration and source inputs remain.
Original and packaged diagnostic hashes are recorded in the runtime manifest.

Local Python acceptance passed **44 packaging/notice/validation tests**, **20
source-acquisition/bundle tests**, and **12 builder contract tests**. These boundary
tests complement native execution; fixtures alone do not establish product
acceptance. Final archives are independently extracted, inventory-checked and
natively launched; their image chunk integrity and decompressed pixels are
checked. Exact package and application hashes are in the linked receipt.

## Fresh public workflow acceptance

On 2026-10-21, public matched-source run
[39561785132](https://github.com/mohamadrezamakanani-dotcom/nova-flux/actions/runs/39561785132)
passed all three native jobs and the cross-platform source aggregation on commit
`cbd04899b78ba81ae2172c08168676b4f9b0e751`. The complete runtime was rebuilt from
the nine locked original archives on every host. Rust/media tests passed
**100 macOS / 78 Windows / 100 Linux**, with zero failures or ignored tests.
Formatting, Clippy, optimized builds, diagnosis, verified packaging and actual
extracted-package rendering also passed. Boundary suites passed 63 script tests,
20 source acquisition/bundle tests and 12 frozen builder contract tests.

All three downloaded application archives and the common source ZIP matched the
run's combined checksums. Independent local extraction rechecked every package
inventory, runtime/source/notice binding and PNG integrity; recomputing the
aggregation locally reproduced its report. The common source ZIP remains
`2d455cac22950eabcadbd23f8e30a147d887516403c51821b1af766d80ca05f5`.
The [retained public build receipt](docs/evidence/public-build-2.3.1.json) describes
these fresh validation artifacts. Existing published v2.3.1 archives, tag and
release receipt were not replaced. CI artifacts expire after one day.

Additional physical Mac folder import, impossible-limit rejection, retry,
two-file conversion, collision preservation, output reveal and settings
restoration passed; see the [manual session receipt](docs/evidence/manual-macos-2.3.1-2026-10-21.json)
and [manual acceptance matrix](docs/MANUAL_ACCEPTANCE.md) for exact scope and
unperformed checks.

## Limits

macOS is locally ad-hoc signed, **not Apple-notarized**. Windows executables are
**unsigned**. Passing tests do not establish every OS/driver, desktop dialog,
physical Windows/Linux computer, processor architecture or perceptual quality.
There is no claim of a universally fastest algorithm or legal certification.
See [platform scope and reproduction](docs/CROSS_PLATFORM.md).

[Historical 2.3.0 validation](docs/validation-2.3.0.md) and
[2.2 validation](docs/validation-2.2.md) are retained separately.
