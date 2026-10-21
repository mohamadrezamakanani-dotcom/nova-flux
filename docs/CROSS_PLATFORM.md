# Cross-platform build and validation

Rust source can target several operating systems. Every executable, codec runtime
and package still belongs to one operating system and processor. An Apple Silicon
`.app` cannot run as a Windows or Linux executable.

## Version 2.3.1 native evidence

| Target | Tested environment | Rust tests | Graphics exercised |
| --- | --- | --- | --- |
| macOS arm64 | Local macOS 27.0.1, Apple M4 | 100 passed, 0 failed/ignored | Metal; actual picker and target-size UI conversion |
| Windows x86_64 | GitHub Windows Server 2022 | 78 passed, 0 failed/ignored | DX12, Microsoft Basic Render Driver / WARP |
| Linux x86_64 | GitHub Ubuntu 22.04 | 100 passed, 0 failed/ignored | X11/Xvfb, Vulkan llvmpipe, Mesa 23.2.1 |

Application/media acceptance passed on source `25529569e1c6d6d24c6979d50fe7e6f97601b046`
for macOS/Linux and `83dc3c6f798b1e030c56f781986ed9cd9fe4f74b` for Windows.
The latter changes build/packaging integration, not Rust application code or the
codec recipe. Original CI runs are 39470592443 (Linux job 118793314030) and
39472802531 (Windows job 118800957336). Their initial packaging results are
separate from source/media acceptance. Final packaging and extracted native rendering passed in run 39513080541 on packaging revision `31b6ab2`.
See [current validation](../VALIDATION.md) for final package evidence.

Native tests include actual codec conversions, geometry/size limits, formatting,
Clippy with warnings denied, optimized builds and bundled runtime diagnosis.
Captured windows use actual framebuffer pixels, not HTML mockups. CI software
adapters do not establish performance or behavior on every physical GPU.

Windows Server evidence does not establish every Windows 10/11 installation or
driver. Ubuntu evidence does not establish every distribution, Wayland compositor
or desktop portal. Linux codec imports use the Ubuntu 22.04 glibc baseline; older
glibc compatibility is not promised. The macOS bundle declares macOS 12 minimum;
this metadata does not prove testing every macOS 12+ release. Intel Macs and
Windows/Linux ARM64 have no validated package. NVENC, Quick Sync and VAAPI are
not implemented paths. Windows/Linux use software codecs; macOS measures
compatible Apple VideoToolbox against CPU encoding.

File/folder selection uses native dialogs. Linux needs suitable XDG desktop
portals. Empty-queue render acceptance does not prove every dialog, drag/drop,
accessibility interaction or file-manager focus on physical Windows/Linux desktops.

## Paths and runtime layout

| Item | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Preferences/GUI lock | `~/Library/Application Support/com.nova.flux` | `%APPDATA%/com.nova.flux` | `$XDG_DATA_HOME/com.nova.flux` or `~/.local/share/com.nova.flux` |
| Cache | `~/Library/Caches/com.nova.flux` | `%LOCALAPPDATA%/com.nova.flux/cache` | `$XDG_CACHE_HOME/com.nova.flux` or `~/.cache/com.nova.flux` |
| Media tools | `Nova Flux.app/Contents/Resources/runtime/bin/` | Beside app: `runtime/bin/` | Beside app: `runtime/bin/` |
| Show output | Finder selection | File Explorer selection | Containing folder via `xdg-open` |

Windows media executables end in `.exe`. Keep the complete extracted package.
Relative XDG values are ignored. `NOVA_RUNTIME` accepts an absolute development
runtime override; installed packages need none. `NOVA_DATA_DIR`/`NOVA_CACHE_DIR`
isolate tests. Available RAM comes from macOS `vm_stat`, Windows
`GlobalMemoryStatusEx`, or Linux `/proc/meminfo`. Extra Windows codec consoles
are suppressed. All hosts use an OS-backed GUI lock; macOS can activate an
existing instance, while Windows/Linux duplicates exit with a clear message.

## Matched source builds and acceptance

The public [matched-source workflow](PUBLIC_RELEASE_BUILD.md) now repeats the
complete native source/runtime/package checks on all three standard hosted
runners. Public [run 39561785132](https://github.com/mohamadrezamakanani-dotcom/nova-flux/actions/runs/39561785132)
passed at `cbd04899b78ba81ae2172c08168676b4f9b0e751`, including extracted native
rendering and common-source aggregation. Independently downloaded packages
passed checksum, inventory, runtime/source and notice verification. Its
[retained receipt](evidence/public-build-2.3.1.json) describes fresh validation
artifacts, separately from the immutable published release record above.

[matched-runtime.yml](../.github/workflows/matched-runtime.yml) builds native
FFmpeg from nine hash-pinned original archives in a private validation repository.
It retains exact source, recipe/tool versions, configure options, compiler-support
notices and build evidence. The same source ZIP must have the same hash on all
three hosts. Package preparation verifies codec sources, target-specific Rust/font
notices, executable imports, every inventory file and privacy boundaries.
Build-time downloads do not become runtime downloads in the installed app.

[repackage-matched.yml](../.github/workflows/repackage-matched.yml) reuses those
exact accepted executable bytes, checks original report revision/status and source
ZIP hash, then packages and safely extracts them on each native host. Extracted
packages run diagnosis and a fresh native rendered-window check. Generated
FFmpeg shell-environment dumps remain private; exact build commands/configuration
and source inputs stay in public packages. No original license/source is rewritten.
Original and packaged diagnostic hashes are recorded.

The older private provider-based workflow and public `--source-only` workflow
remain separate. Public source CI uploads reports/logs/PNGs, never provider media
binaries or generated input media. Historical 2.3.0 acceptance is preserved in
[its record](validation-2.3.0.md) and [receipt](evidence/portable-2.3.json).

## Reproduce

Use Rust 1.99.0 (`rustfmt`, `clippy`, `rust-docs`), Python 3.12 and a native
compiler/linker. Follow [BUILD.md](../scripts/runtime-source/BUILD.md) and the
matched workflow for tool dependencies. Windows additionally needs MSYS **Perl**
for libvpx and the MSVC SDK/toolchain for the Rust application. Build the matched
runtime, collect target notices, and use a fresh validation directory:

```sh
python scripts/run_validation.py --platform linux-x86_64 --runtime runtime --report-dir validation/linux-local --toolchain 1.99.0 --source-only
```

Use `windows-x86_64` or `macos-arm64` on its native host. On headless Linux,
run under Xvfb with the workflow's Vulkan llvmpipe environment. Windows CI uses
DX12 WARP via `NOVA_WGPU_FORCE_FALLBACK=1`; normal desktop launches need none of
these CI overrides. `--source-only` stops before old private-provider packaging;
complete release packaging uses [package_verified.py](VERIFIED_PACKAGING.md).

`--smoke-test` requests an actual WGPU framebuffer, at least three frames and
graceful shutdown. Missing/corrupt screenshots fail acceptance. This is narrower
than manual physical-desktop acceptance. macOS is ad-hoc signed, not notarized;
Windows executables are unsigned. Signing and OS coverage are independent of
passed source/runtime/package integrity checks.
