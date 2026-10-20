# Retained FFmpeg runtime source bundle

`package_sources.py` creates a separate public source ZIP from the exact nine
archives selected by `sources.lock.json`. It supports source distribution before
all platform builds finish: original sources and the build recipe do not require
a platform binary or a successful CI run. Publishing the ZIP is a separate step.

## Create and inspect

Run from the Nova repository root with Python 3.10 or later:

```sh
python3 scripts/runtime-source/package_sources.py \
  --cache ../runtime-source-cache \
  --destination ../runtime-source-distribution \
  --output ../nova-ffmpeg-runtime-sources.zip

python3 scripts/runtime-source/package_sources.py \
  --verify --output ../nova-ffmpeg-runtime-sources.zip
```

Creation requires all three paths. The cache must contain `archives/` with every
locked original archive. Cached extracted trees and copied license files are not
trusted as source inputs: license text is read directly from the hash-verified
original archives. An unused research archive such as FFmpeg 9.0.2 is excluded.
No package installation, source acquisition, compilation or upload is performed.

The staging parent receives `<source_set_id>-sources/`, and the ZIP contains that
same single top folder. Both are verified before delivery. Repeating the command
with the same inputs produces the same ZIP bytes. Existing stage/output content
is accepted only when identical; changed content is preserved and causes failure.
Choose new paths for a revised recipe rather than overwriting reviewed artifacts.

Use repeatable `--build-manifest PATH` to include native `build-manifest.json`
files for completed platforms. Each must identify `macos-arm64`, `linux-x86_64`
or `windows-x86_64`, the same source set, and exactly the bundled builder and
lock SHA-256 values. Duplicate platforms, changed recipes/locks and patches are
rejected. A base ZIP without platform manifests remains complete for building.

## Contents and licenses

- `archives/`: all nine full upstream archives, with the original bytes intact.
- `licenses/<component>/`: every license/notice text listed in the lock, with
  original relative paths and verified hashes.
- `COPYING.GPLv3`: FFmpeg's full original GPL v3 text, duplicated for visibility.
- `LICENSE`: Nova's own MIT license, applying to its own code and scripts.
- `scripts/runtime-source/`: the source lock, acquisition/build/package drivers,
  `BUILD.md`, this guide and focused source/package integrity tests.
- `INTRODUCTION.md`: source-set identification, dependency license summary,
  offline verification and native build instructions.
- `BUNDLE-MANIFEST.json`: every payload file's path, byte length and SHA-256;
  recipe/source-set digests; exact builder/lock digests; optional platform list.
- `build-manifests/`: optional exact-byte native build records supplied at creation.

The FFmpeg runtime is configured with `--enable-gpl --enable-version3`, excluding
nonfree components. Its combined runtime license is GPL-3.0-or-later. Root MIT
`LICENSE` does not replace the dependency terms. Complete archives retain all
additional upstream copyright headers and files. The lock provides source URLs,
versions/references, archive integrity values and the license-text inventory.

Native compilers, OS SDKs/system libraries and build tools are prerequisites.
Their binaries are not in this source bundle. `BUILD.md` specifies the native
toolchain, platform constraints, exact configuration recipe, compiler runtime
boundary, and separate acceptance checks. No upstream source patches are applied;
the current packager rejects a nonempty patch list.

## Verify after download and rebuild

An integrity check with a separately published ZIP checksum:

```sh
python3 scripts/runtime-source/package_sources.py \
  --verify --output ../nova-ffmpeg-runtime-sources.zip \
  --expected-sha256 PUBLISHED_ZIP_SHA256
```

`--verify` only reads the ZIP; it needs neither a source cache nor a staging
directory. It rejects changed/missing/unlisted files, unsafe ZIP paths, links,
duplicate/case-colliding paths, missing dependency archives/license texts, and
native build manifests that do not match the retained source/recipe. Failure
returns a nonzero exit status. The verifier also checks original archive and
license hashes against the bundled lock, independently of the file inventory.

After unpacking, use its top folder as the working directory:

```sh
python3 scripts/runtime-source/acquire_sources.py \
  --cache-dir . --offline --verify-only
python3 scripts/runtime-source/build_runtime.py --self-test
python3 scripts/runtime-source/build_runtime.py \
  --platform macos-arm64 --sources . \
  --work-dir ../nova-native-build --destination ../nova-native-runtime --jobs 8
```

Choose `linux-x86_64` or `windows-x86_64` on those native hosts. Windows uses
MSYS2 UCRT64 and its native Python/compiler; `python` may be the command name.
The build work/output paths must be new or empty, as specified in `BUILD.md`.

The ZIP uses sorted file paths, fixed 1980 timestamps, fixed ordinary permission
bits, no ZIP comments, and stored (uncompressed) entries. Upstream compressed
archives are already compact. Recipe/source-set digests are SHA-256 of compact
UTF-8 JSON (`ensure_ascii=False`, `sort_keys=True`, separators `(',', ':')`)
containing `{path, bytes, sha256}` records sorted by path. Recipe records cover
the exact lock, acquisition driver, build driver and `BUILD.md`; source-set
records cover the nine original archives. Optional native manifests leave these
two digests unchanged but change the complete ZIP checksum.

The manifest excludes itself to avoid a circular hash. An internal consistency
check cannot authenticate an archive whose contents and manifest were both
replaced: obtain the published ZIP SHA-256 through a trusted release channel.
Hash verification is not a digital signature, legal certification, guarantee of
binary-identical output across toolchains, or proof of native/application/release
acceptance. Those claims require their separate evidence.

Focused packager tests:

```sh
python3 -m unittest discover -s scripts/runtime-source -p 'test_package_sources.py' -v
```
