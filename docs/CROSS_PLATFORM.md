# Cross-platform build and validation

Rust source can target several operating systems. A compiled application and its
FFmpeg runtime still belong to one operating system and processor architecture.
An Apple Silicon `.app` cannot be used as a Windows or Linux executable.

The existing delivered build and runtime were validated on Apple Silicon macOS.
Windows and Linux support is being established through native CI, with separate
builds, real media conversion tests, rendered native windows and portable runtime
packages. A workflow definition or a cross-compile is not a passing native run.
For a particular revision, use that revision's successful `report.json` and logs
as evidence; do not infer results from this document.

## Native targets and limits

| Target | Native CI environment | Graphics exercised | Distribution status |
| --- | --- | --- | --- |
| Apple Silicon macOS | Existing local Mac acceptance | Metal, CPU and Apple VideoToolbox | Existing arm64 app; changes require the Mac regression checks |
| Windows x86_64 | GitHub `windows-2022` | DX12 software/WARP adapter with real wgpu rendering | Private test ZIP after native checks pass |
| Linux x86_64 | GitHub `ubuntu-22.04` | X11/Xvfb with Mesa lavapipe Vulkan | Private test tar.gz after native checks pass |

Windows Server 2022 evidence does not establish testing on every Windows 10/11
installation or graphics driver. Ubuntu 22.04 evidence does not establish every
distribution, Wayland compositor or GPU. The runtime provider's minimum OS is
recorded in `scripts/portable_runtime.lock.json`; that minimum does not prove the
GUI supports the same baseline. Intel Macs and Windows/Linux ARM64 have no
validated package or pinned runtime in this matrix. NVENC, Quick Sync and VAAPI
are not implemented acceleration paths; Windows/Linux use the tested CPU path.

The UI uses egui/eframe and wgpu, with X11/Wayland backend features enabled for
Linux. File/folder selection uses native dialogs; Linux requires an appropriate
XDG desktop portal for its default dialog backend. The smoke check renders a
window and captures its framebuffer; it does not claim manual dialog, drag/drop,
accessibility, file-manager focus or every desktop interaction was tested.

## Platform paths and runtime layout

| Item | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Preferences and GUI lock | `~/Library/Application Support/com.nova.flux` | `%APPDATA%/com.nova.flux` | `$XDG_DATA_HOME/com.nova.flux` or `~/.local/share/com.nova.flux` |
| Cache | `~/Library/Caches/com.nova.flux` | `%LOCALAPPDATA%/com.nova.flux/cache` | `$XDG_CACHE_HOME/com.nova.flux` or `~/.cache/com.nova.flux` |
| Media tools | `Contents/Resources/runtime/bin/ffmpeg`, `ffprobe` | Beside app: `runtime/bin/ffmpeg.exe`, `ffprobe.exe` | Beside app: `runtime/bin/ffmpeg`, `ffprobe` |
| Show output | Finder selection | Native File Explorer selection | Open containing folder with `xdg-open` |

The Mac folders are preserved. Relative XDG directory values are ignored.
`NOVA_RUNTIME` selects a development runtime. `NOVA_DATA_DIR` and `NOVA_CACHE_DIR`
accept absolute paths for isolated acceptance runs; they are not required in an
installed package. Windows uses physical available RAM through
`GlobalMemoryStatusEx`; Linux reads `/proc/meminfo`; macOS uses `vm_stat`.
Codec/inspector subprocesses suppress extra Windows console windows.

All platforms enforce an OS-backed GUI file lock, released on process exit or
crash. Native foreground activation of the existing instance is implemented on
macOS. The other platforms currently exit a duplicate with a clear message;
foreground activation on those desktops is not claimed.

## Private validation and source-only CI

`.github/workflows/portable-validation.yml` runs only while the repository is
private. It uses the hash-pinned runtime lock, whose upstream download size and
SHA-256 are checked before extraction. Archive traversal, special files,
architecture, required CPU encoders, the original license and the complete
runtime file manifest are checked. Runtime acquisition happens only at build/test
time; the installed app does not download codecs.

Each native job runs:

1. Python boundary tests for extraction, provenance and render-evidence checks.
2. Rust 1.99.0 formatting, all-target Clippy with warnings denied, and all-target
   real-media tests with ignored acceptance tests explicitly included.
3. An optimized release build and its `--diagnose` JSON/version contract.
4. A native `--smoke-test` run, at least three rendered frames, a matching version
   and host event, and a complete RGB/RGBA PNG with valid chunks and pixel data.
5. Private portable packaging, staged-runtime diagnosis without `NOVA_RUNTIME`,
   and another native render/screenshot from the staged application.

The runner streams command output, records exit codes and deadlines, writes a
per-step log and a final JSON report, and fails on missing evidence. App state
and cache are isolated under the run's report directory. Test counts can differ
by OS: Unix-only shell instrumentation is deliberately excluded on Windows,
while portable conversion/preset/geometry tests still run there. The JSON report
records the actual count; one platform's count is not reused as another's proof.

`--source-only` runs the same formatting, Clippy, real media, release diagnosis
and native window checks, then stops before packaging. The public
`.github/workflows/source-validation.yml` uses this mode and uploads only the
report, command logs and UI PNGs. It does not upload application executables,
FFmpeg binaries, runtime archives, generated source media or private test packages.
Both workflows pin checkout and artifact-upload actions to exact commits verified
through GitHub's release/tag API: checkout v7.0.1 and upload-artifact v7.0.2.
Evidence and private binary artifacts expire after one day. These are CI test
artifacts, not GitHub Releases.

The Windows/Linux runtime lock is currently configured for private validation packages
only. Complete corresponding source for linked FFmpeg dependencies has not been
assembled and verified. Public binary packaging stays blocked until that work is
complete; the Rust application's MIT license does not replace component licenses.
Publishing the source does not publish the private runtime artifacts.

## Reproduce on the matching native host

Install Rust 1.99.0 with `rustfmt` and `clippy`, Python 3.10 or newer, and a native
linker/toolchain. Windows builds use Visual Studio's C++ build tools and Windows
SDK. Linux needs the X11/Wayland development libraries, a usable Vulkan/GLES
driver, and the desktop services appropriate to native dialogs. The workflow
lists the Ubuntu packages used for its native baseline.

On Windows, from a fresh source checkout:

```powershell
rustup toolchain install 1.99.0 --profile minimal --component rustfmt --component clippy
$env:WGPU_BACKEND = 'dx12'
$env:NOVA_WGPU_FORCE_FALLBACK = '1'
python scripts/run_validation.py --platform windows-x86_64 --runtime runtime --report-dir validation/windows-local --toolchain 1.99.0 --private-ci
```

On Ubuntu with the workflow's dependencies installed:

```sh
rustup toolchain install 1.99.0 --profile minimal --component rustfmt --component clippy
WGPU_BACKEND=vulkan LIBGL_ALWAYS_SOFTWARE=1 VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json xvfb-run -a -s '-screen 0 1440x1000x24' python3 scripts/run_validation.py --platform linux-x86_64 --runtime runtime --report-dir validation/linux-local --toolchain 1.99.0 --private-ci
```

Use a new report directory for each run. Replace `--private-ci` with
`--source-only` to run the source validation checks without packaging. Normal
hardware-backed desktop launch does not require the CI software-adapter variables.
To smoke an existing binary separately, set `NOVA_SMOKE_TEST=1`,
`NOVA_SMOKE_TEST_MS=3000` and an absolute `NOVA_SMOKE_SCREENSHOT` PNG path, or pass
`--smoke-test`. A screenshot is required by the validation runner, and a missing
or failed native completion is an error.
