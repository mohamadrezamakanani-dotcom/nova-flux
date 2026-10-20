# Contributing to Nova Flux

Useful contributions include reproducible bug reports, clearer documentation,
accessibility feedback, focused fixes, and native platform testing. Read the
[support guide](SUPPORT.md) before reporting a problem and use the
[private security route](SECURITY.md) for a suspected vulnerability. Follow the
[code of conduct](CODE_OF_CONDUCT.md) in issues, reviews, and pull requests.

## Start with a focused change

Search existing issues first. For a substantial feature or behavior change,
describe the user problem and proposed scope in a feature request before building
it. Small fixes and documentation corrections can go straight to a pull request.
Keep unrelated cleanup separate and explain any changed user behavior.

Use generated, redistributable test media. Do not commit personal media,
credentials, private paths, raw environment dumps, build output, or runtime
binaries. Application source is MIT licensed; dependencies and contributed assets
retain their applicable notices. Include provenance and license information for
new third-party material.

## Development and checks

Use Rust **1.99.0** with locked dependencies. CI uses Python **3.12**; release
helpers require Python 3.11 or newer. Install native desktop/compiler dependencies
for your host as recorded in [cross-platform guidance](docs/CROSS_PLATFORM.md)
and the workflows. From the repository root:

```sh
rustup toolchain install 1.99.0 --profile minimal --component rustfmt --component clippy --component rust-docs
cargo +1.99.0 fmt --all -- --check
cargo +1.99.0 test --locked --no-default-features
cargo +1.99.0 clippy --all-targets --locked -- -D warnings
cargo +1.99.0 build --release --locked
python3 -B -m unittest discover -s scripts -p 'test_*.py'
python3 -B -m unittest discover -s scripts/runtime-source -p 'test_*.py'
python3 scripts/runtime-source/build_runtime.py --self-test
```

In PowerShell use your Python 3.12 interpreter's command, usually `python`, in
place of `python3`. Windows application builds use the MSVC toolchain. Windows
distribution builds additionally require `-C target-feature=+crt-static`; the
codec runtime is built separately with MSYS2 UCRT64.

The repository does not include FFmpeg/ffprobe executables. For real media tests,
build your host's runtime from the [retained-source recipe](scripts/runtime-source/BUILD.md).
The runtime directory contains `bin/ffmpeg` and `bin/ffprobe` (`.exe` on Windows).
Set both overrides to its **absolute directory**, not to its `bin/` subdirectory:

```sh
export NOVA_RUNTIME=/absolute/path/to/runtime-macos-arm64
export NOVA_TEST_RUNTIME="$NOVA_RUNTIME"
cargo +1.99.0 test --all-targets --locked -- --include-ignored --test-threads=1
target/release/nova-flux --diagnose
```

Use the runtime built for `linux-x86_64` on Linux. In Windows PowerShell, after
building its runtime into `runtime-windows-x86_64`:

```powershell
$env:NOVA_RUNTIME = (Resolve-Path .\runtime-windows-x86_64).Path
$env:NOVA_TEST_RUNTIME = $env:NOVA_RUNTIME
cargo +1.99.0 test --all-targets --locked -- --include-ignored --test-threads=1
.\target\release\nova-flux.exe --diagnose
```

The first test command without default features is a narrower library check.
Only the run with the native runtime and `--include-ignored` exercises real media
acceptance. Report the actual counts and ignored tests; a skipped test is not a
pass. The recorded 2.3.1 native counts are macOS **100**, Windows **78**, and Linux
**100**. Windows excludes Unix-only instrumentation. See [VALIDATION.md](VALIDATION.md)
for the tested revisions and environments.

For GUI or file-dialog changes, also follow the [manual checklist](docs/MANUAL_ACCEPTANCE.md).
CI framebuffer capture alone does not establish manual interaction on physical
Windows/Linux desktops. For packaging changes use the
[verified packaging gates](docs/VERIFIED_PACKAGING.md), including extraction and
native launch, and retain a sanitized receipt.

## Preserve the release contract

Successful video conversion keeps original dimensions, cadence, and full selected
duration, and stays at or below the applied byte maximum. Failed or cancelled
work must not publish an invalid output or overwrite an original or existing
output. Use the production profile for measurements, preserve cancellation, and
add a focused regression test when behavior changes.

The 2.3.1 source archives, source ZIP, recipe, and released assets are frozen.
Do not revise a release asset in place. A change to the codec builder, source
lock, acquisition recipe, or retained build instructions requires a new matching
source bundle and native runtime rebuild and acceptance on **all three targets**,
with fresh notices and package inventories. Do not present prior receipts as
evidence for changed runtime bytes. Dependency changes also need updated notices
and appropriate native validation.

## Pull request evidence

Explain the problem, resulting behavior, and checks actually run. Include the
application version/commit, native OS/architecture, runtime provenance, and exact
test results where relevant. Mark missing checks `NOT_RUN` and explain why.
Use the [pull request template](.github/pull_request_template.md); include
sanitized before/after captures for visible changes. A review or merge does not
guarantee a release date or compatibility with untested systems.
