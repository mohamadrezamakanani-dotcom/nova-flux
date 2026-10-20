# Rust and embedded-font binary notices

Run the notice collector for the same locked dependency graph, Cargo features,
Rust toolchain, and target used to build each release. It collects original
license, copyright, NOTICE, exception, and font texts; it does not compile the
application. Python 3.11 or newer is required.

```sh
cargo +1.99.0 fetch --locked --target aarch64-apple-darwin
rustup component add rust-docs --toolchain 1.99.0
python3 scripts/collect_binary_notices.py \
  --target aarch64-apple-darwin --toolchain 1.99.0 \
  --output dist/notices-aarch64-apple-darwin
```

Use `x86_64-pc-windows-msvc` or `x86_64-unknown-linux-gnu` on the corresponding
native release runner. Use an output directory that does not already exist.
Include the **entire output directory** in each distributed application archive.
Record its `manifest.json` alongside the binary's build and checksum evidence.

The collector never fetches Cargo crates implicitly. `cargo fetch` above is an
explicit preparatory operation; it may download target-specific packages. The
optional `--fetch-upstream` permits only small license files and repository tree
metadata from public GitHub revisions recorded in the exact crate archive.
Downloads are bounded at 4 MiB per request and cached in
`scripts/notices-upstream`; `--upstream-cache` can select another location.
The repository retains the exact 55 public upstream license/tree files used by
all three locked target graphs (about 4.1 MB), so normal release collection is
offline. `cache-manifest.json` records each original URL, filename, size and
SHA-256; changed retained cache content causes failure. Without the fetch flag,
an absent cache entry causes failure. No package-manager
installation, build, or credentials are required by this script.

## What the manifest proves

The collector runs `cargo metadata --locked --offline --filter-platform TARGET`
and `cargo tree --locked --offline --target TARGET --edges normal,build`. The tree
is saved with its hash. Cargo's generated absolute root-package location is
normalized to `{SOURCE}` before hashing/writing, preserving package versions and
dependency edges without publishing a private checkout path. Original upstream
license/source bytes are not normalized. Development dependencies and crates solely for other
targets are excluded. Build scripts and procedural macro dependencies are
conservatively included: this is the selected build graph, rather than a claim
that every listed crate contributes bytes to the final executable.

For each selected registry package the collector verifies the cached `.crate`
archive against its exact `Cargo.lock` SHA-256, then reads notice bytes from that
archive rather than trusting a potentially edited unpacked source directory.
The manifest records name, version, registry, checksum, license expression,
chosen license branches, resolved features, individual file hashes, and any
upstream supplement's URL, Git revision and verified Git blob ID.

Some workspace crates omit their parent repository's license files from their
published package. The collector retrieves those notices from the immutable Git
revision in `.cargo_vcs_info.json`, preserving applicable root/ancestor and crate
subdirectory notices. It does not use a moving branch's license as evidence for
an older release. Copyright-bearing initial Rust/C/XML/IDL source comments are
retained verbatim as supplementary attribution.

The installed Rust toolchain's complete `COPYRIGHT.html`, standard-library
`COPYRIGHT-library.html`, and accompanying license texts are also preserved and
hashed. `rustc -vV` identifies the toolchain and host. Missing matching toolchain
notice files cause failure; install that toolchain's `rust-docs` component.
Keeping the complete inventory is conservative and does not mean every compiler
component was linked into the application.

Unknown license expressions, unreviewed package sources, absent or changed
archive checksums, missing notice text, empty notices, unsafe archive paths,
changed upstream blob contents, and missing font notices cause failure. Output
is created only after all packages pass; existing output is never overwritten.
The finite reviewed expression list must be reassessed when dependencies change.

## License choices and fonts

The current graph permits MIT, Apache 2.0, BSD, ISC, Zlib, Boost, Unicode and font
license choices. `OR` branches select the recorded permissive choice; an `AND`
requires every listed license. All available original notices are retained even
when only one alternative is selected. In particular, an available GPL or LGPL
alternative does not mean it is the selected license for a dual-licensed crate.
This follows the [Cargo license-field definition](https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields).

`epaint_default_fonts` embeds four font families. The collector requires the
exact `fonts/Hack-Regular.txt` (including Hack/Bitstream Vera terms),
`fonts/OFL.txt` (Noto Emoji), `fonts/UFL.txt` (Ubuntu), and
`fonts/emoji-icon-font-mit-license.txt`, together with the crate's own MIT/Apache
notices. The crate's SPDX expression alone does not enumerate the Hack notice.
Font names and copyright notices are preserved; this process does not rename or
modify font files. macOS system fonts read from the installed operating system
are not copied by this collector or treated as redistributable project assets.

`khronos_api` contains registry XML/IDL with additional original notices. Its
complete published source is retained, and its missing ANGLE BSD license is
retrieved from the exact submodule commit recorded in the parent repository.
This preserves the original Khronos/ANGLE notices that a crate-level Apache
identifier alone would overlook.

## Reviewed upstream text omissions

The exact `dispatch` 0.2.0 archive
`bd0c93bb4b0c6d9b77f4435b0ae98c24d17f1c45b2ff844c6151a07256ca923b`
and its recorded upstream commit
`82d6c7a5b75dc0c71c3f46f87bb6c16a476f7748` contain an explicit `license = "MIT"`
declaration and `authors = ["Steven Sheldon"]` in the original `Cargo.toml`, but
no separate LICENSE or copyright notice. Missing a separate file does not erase
that upstream license declaration.

For this checksum-pinned version only, the bundle retains every original crate
file, including the manifest, README and complete original sources. A separately
labeled supplement contains the canonical MIT permission, conditions and
warranty text from [SPDX's pinned MIT text](https://github.com/spdx/license-list-data/blob/31ba1a50e5397e00a304dbadc76531740e89ee48/text/MIT.txt).
It omits the generic template copyright placeholder rather than inventing an
author copyright notice or year. `notice_omission` records this treatment in the
manifest; `UPSTREAM-NOTICE-OMISSION.txt` explains it inside the distributed bundle.

This is a documented exception rather than a silent general fallback. The
original author attribution is retained exactly as published; it is not
converted into a fabricated copyright statement. The approach is consistent
with Cargo's meaning of its license field and
[cargo-about's canonical-text fallback](https://github.com/EmbarkStudios/cargo-about/blob/main/src/generate.rs).
A new dispatch version or checksum requires fresh review.

Ten checksum-pinned modern `objc2` family crate versions also supply an upstream
`LICENSE.md` that explicitly grants MIT by reference but omits its permission
text. `MIT_TEXT_OMISSIONS` records these exact name/version/checksum tuples. For
those versions the collector preserves their entire original source and actual
upstream `LICENSE.md`, including its Apple SDK discussion, and supplies the same
separately labeled canonical MIT permission text. It does not replace or conceal
the upstream terms. An unreviewed MIT crate without the selected permission
text fails, even if it contains a file named LICENSE. All other missing or
unknown license cases still fail.

## Release evidence and scope

A local collection on 20 October 2026 selected 154 registry crates for Apple
Silicon macOS, 155 for Windows x86-64, and 268 for Linux x86-64. All three target
collections completed using Cargo's target-filtered graph on the macOS host.
These counts are evidence for the current lockfile, not permanent requirements;
each native release runner must collect its own graph and toolchain notices.
The focused boundary tests run with:

```sh
python3 -m unittest discover -s scripts -p test_binary_notices.py
```

A successful collection proves that this Rust/font notice inventory was built
from the recorded materials. Before distributing a binary, separately inspect
its actual native library closure and include the matching notices/source
materials for FFmpeg and any redistributed native libraries. System libraries,
platform SDK terms, executable signing, binary behavior, and matching codec
corresponding source are separate release checks. A Cargo graph or successful
notice collection alone does not prove those checks passed.
