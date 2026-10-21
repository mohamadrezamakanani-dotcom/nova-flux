# Repeat native matched-source validation

The manually dispatched [Public matched-source validation workflow](../.github/workflows/public-matched-validation.yml)
builds a fresh Nova application and its complete media runtime on three native
GitHub-hosted runners. It produces temporary validation artifacts for review.
It does not publish a GitHub Release or replace an existing release asset.

The existing v2.3.1 release assets and their recorded hashes are immutable
historical outputs. A new run validates its selected source commit with the
tools currently installed on its runners; it does not reproduce or amend that
earlier release record. Matching source archives are deterministic, while
native compiler, SDK and system-library changes can change application and
runtime binary bytes. See [verified packaging](VERIFIED_PACKAGING.md) for the
package integrity contract and the existing release evidence.

## Accepted public run

[Run 39561785132](https://github.com/mohamadrezamakanani-dotcom/nova-flux/actions/runs/39561785132)
passed all three native jobs and aggregation on 2026-10-21, using commit
`cbd04899b78ba81ae2172c08168676b4f9b0e751`. Rust/media tests passed
100 on macOS, 78 on Windows and 100 on Linux, with none failed or ignored.
All four downloaded archives matched the combined checksums; package inventories,
original sources, notices and rendered PNGs were independently checked locally.
See the [retained acceptance receipt](evidence/public-build-2.3.1.json).
This successful repeat did not replace the published v2.3.1 release assets.

## Start a run

In GitHub, open **Actions → Public matched-source validation → Run workflow**
and select the branch or tag to validate. Every dispatch builds all three
platforms. The workflow is restricted to public repositories and needs only
read access to repository contents. It does not need repository secrets.

The GitHub CLI equivalent, from a checkout of the intended public repository:

```sh
gh workflow run public-matched-validation.yml --ref main
```

Use the Actions page to find the resulting run and download its artifacts
before their one-day retention period expires. Dispatching from a fork uses
that fork's workflow and source; it does not affect the upstream release.
For another attempt of the same run, use **Re-run all jobs** so all three
receipts and the common source artifact share the new attempt number.

## Native hosts and checks

| Platform | Standard runner | Application toolchain | GUI acceptance |
| --- | --- | --- | --- |
| macOS arm64 | `macos-14` | Rust 1.99.0, Apple native linker | System Metal adapter |
| Windows x86_64 | `windows-2022` | Rust 1.99.0 MSVC, static CRT | DirectX 12 software adapter |
| Linux x86_64 | `ubuntu-22.04` | Rust 1.99.0, native GNU linker | Xvfb and Vulkan llvmpipe |

The workflow verifies the actual host OS and architecture before building.
GitHub documents the standard runner labels in its
[hosted-runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
No larger runner or scheduled build is configured. Each native job has a
90-minute limit. macOS must pass the real rendered GUI checks; unavailable
Metal or desktop rendering fails the job rather than becoming an assumed pass.

Each job:

1. Installs Python 3.12, the exact Rust 1.99.0 toolchain, rustfmt, Clippy and
   `rust-docs`, plus native build and desktop dependencies.
2. Runs the source-acquisition/source-bundle boundary tests and the frozen
   native builder's self-tests.
3. Acquires and verifies all nine original archives from the source lock,
   builds the eight codec libraries and FFmpeg/ffprobe, and runs the builder's
   architecture, import, licensing, filter, encoder and real-media checks.
4. Fetches the locked Cargo dependencies and collects target-specific Rust,
   embedded font and compiler/toolchain notices.
5. Runs every `scripts/test_*.py` check, formatting, Clippy with warnings as
   errors, every Rust test with ignored tests included, the optimized build,
   native runtime diagnosis and a rendered native GUI smoke test.
6. Builds the corresponding-source ZIP, then runs the verified packager's
   source/runtime/notice/import/privacy/inventory gates and native diagnosis.
7. Independently extracts the final archive with bounded, checked extraction,
   verifies its inventory, diagnoses it without a runtime override, and captures
   its rendered native GUI.
8. Exports compact public evidence and binds the accepted archive to its
   corresponding-source hash.

The workflow uses the unchanged recipe in
[`scripts/runtime-source/BUILD.md`](../scripts/runtime-source/BUILD.md). Windows
codec compilation runs in MSYS2 UCRT64 with native GCC/Python, Perl and the
required native tools. Windows application compilation and packaging then run
in the native MSVC host shell. `objdump` is resolved from the installed UCRT64
tools, including GitHub's runner temporary directory. Rust embeds generic
source/home path mappings on every platform; Windows additionally uses
`+crt-static`.

## Artifacts and review

The version comes from `Cargo.toml`; archive names contain that version:

| Artifact | Contents |
| --- | --- |
| `matched-package-<platform>-<attempt>` | One final `Nova-Flux-<version>-<platform>.zip` or Linux `.tar.gz`, plus its `.sha256` file |
| `matched-evidence-<platform>-<attempt>` | Allowlisted `validation.json`, `receipt.json`, and completed native/extracted GUI PNGs |
| `matched-common-sources-<attempt>` | One `Nova-Flux-<version>-FFmpeg-sources.zip` and its `.sha256` file, uploaded by Linux only |
| `matched-summary-<attempt>` | Combined cross-platform `report.json` and `SHA256SUMS` for the three application archives and common source ZIP |

Each application archive already includes the original corresponding media
sources and notices. The separate source ZIP provides the common source
download. It is created independently on each host without platform build
manifests, so its bytes must match across platforms.

After all three native jobs succeed, a final aggregation job downloads this
run's three exact evidence artifacts and the common source ZIP. It checks three
unique successful platforms, the current source commit and Cargo version,
nonempty passing test runs with no failures or ignored tests, identical source
ZIP hashes/sizes, and the checked-out frozen builder, source lock and source-set
identity. It independently hashes the downloaded common source ZIP. A mismatch
fails the workflow instead of leaving source comparison as a manual step.

Before treating a run as complete, confirm **all three native jobs and the
aggregation job succeed**, then download `matched-summary-<attempt>` and check
the final application archives against its `SHA256SUMS`. The aggregation job
records native-job archive hashes; it does not download or re-hash application
packages. Their byte, inventory, diagnosis and rendered GUI checks were
performed upstream on each native host. Each native receipt also records the
package-manifest hash, test counts and GUI image hashes. An incomplete or
failed native job cannot upload an accepted final package. A partial evidence
artifact may still be available to show which checks ran.

The allowlisted summaries omit command arguments, absolute host paths, full
diagnosis payloads and raw error/log text. The workflow uploads no raw runtime
directory, build tree/logs, FFmpeg environment dump, test media, extracted
application tree or Cargo target directory. The verified package retains its
own normalized provenance and original notices under its existing contract.

Artifacts expire after **one day**. One complete run is approximately
250–300 MB of final packages, the common source ZIP and compact evidence;
actual archive sizes can change. No compiler cache is stored. Save the
artifacts needed for review before expiry, and compare their published
checksums after downloading. Hash consistency does not establish code signing,
notarization, identical binaries across toolchains, or behavior on every
physical machine.

The separate **Source validation** workflow remains the automatic Windows and
Linux source/media/GUI check. It uses its existing hash-pinned prebuilt runtime
and produces no matched-source release package. Its badge describes that
limited scope. Documentation and screenshot-only changes do not rebuild the
application; code, test, build-script, asset and workflow changes still do.
