# Windows and Linux native validation packages

These scripts acquire a pinned **build-time** media runtime and produce
**private CI validation artifacts**. They do not establish a public binary
release. The app performs no runtime download.

Run on an x86_64 native Windows or Linux runner with Python 3.10 or newer:

```text
python scripts/test_portable_packaging.py
python scripts/fetch_portable_runtime.py --platform windows-x86_64 --destination runtime --cache .runtime-downloads
cargo build --release --locked
python scripts/package_portable.py --platform windows-x86_64 --runtime runtime --binary target/release/nova-flux.exe --destination dist --channel private-ci
```

For Linux replace `windows-x86_64` with `linux-x86_64` and use
`--binary target/release/nova-flux`. The resulting folder keeps `runtime/`
beside the application. The package runs its own `--diagnose` with
`NOVA_RUNTIME` removed, checks required CPU encoders and executable
architecture, rejects an app version that differs from `Cargo.toml`, verifies
all runtime hashes, then writes a ZIP (Windows) or
tar.gz (Linux) plus a SHA-256 file. Nothing is uploaded or published.

For real engine acceptance set `NOVA_TEST_RUNTIME` to the absolute acquired
`runtime/` directory, then run the project's ignored media tests. A successful
package diagnosis only proves runtime discovery and binary availability;
real conversion and GUI acceptance remain separate checks.

## Exact upstream provenance

`portable_runtime.lock.json` pins BtbN/FFmpeg-Builds release
`autobuild-2026-10-19-13-06`, the published byte lengths and SHA-256 digests,
FFmpeg revision `46d899d7a02bc63237472cc4897258d5ddf274ac`, and builder
revision `63ac3265b0789ea16c7b1f8351cd2ad195a28722`.
Acquisition fails on an unavailable asset, unexpected checksum, changed
version, nonfree build or missing encoder. It never silently switches to
`latest`. Upstream retains daily releases for a limited time; retain verified
archives in a private build cache or review and update the lock explicitly
when upstream retires this tag.

The [upstream builder documentation](https://github.com/BtbN/FFmpeg-Builds/tree/63ac3265b0789ea16c7b1f8351cd2ad195a28722)
describes these as static GPL Windows/Linux builds. Its declared media-runtime
baseline is Windows 10 22H2 with UCRT, or Linux glibc 2.28 and kernel 4.18.
Those are **FFmpeg provider baselines**, not promises about Nova Flux GUI
compatibility. The actual native CI runner and tested hardware define the
evidence; do not advertise all Windows versions or Linux distributions.

Both platforms require x86_64. These scripts reject ARM64 and 32-bit binaries.
They preserve the upstream `LICENSE.txt`, documentation, presets, and any
runtime dependencies. They remove unused `ffplay` only, and add source URLs,
original archive digest, per-file digests, executable version/configuration
output, FFmpeg license output and an explicit private artifact notice.

## Public distribution status

The builder's MIT license covers its build scripts. It does not turn the
GPL-enabled FFmpeg binaries into MIT software. The source and builder commit
links identify provenance but do not prove that complete corresponding
source for all linked dependencies is available in this package.

The upstream binary archives do not supply a verified complete corresponding
source bundle. Public runtime/binary redistribution is therefore blocked in
these scripts. Assemble and verify the exact FFmpeg and dependency sources,
patches, configuration and build instructions before changing this policy.
See [FFmpeg's own license and source guidance](https://ffmpeg.org/legal.html).
Publishing the Rust source without the third-party runtime is a separate
release path; do not attach these private validation binaries to it.
