# Matched codec sources and binary release plan

User authorization: complete professional source/license preparation, build and verify before public application packages. Application code remains MIT; third-party licenses remain intact. Existing installed2.3.0 and original Nova Converter are preserved until replacement passes acceptance.

## Chosen design

Replace opaque provider runtime closures with our own natively built FFmpeg8.1.3 and nine hash-pinned source archives (FFmpeg,x264,LAME,ogg,vorbis,Opus,libvpx,dav1d,zlib). Disable external autodetection/networking, explicitly enable required codecs, retain native built-in file decoders/filters, and preserve Apple VideoToolbox through system frameworks. Link third-party libraries statically; audit actual imports against platform system-library allowlists. No nonfree components.

Preserve unmodified original source archives, exact original hashes, any applied patches, build scripts, configure options and tool versions. Publish this corresponding-source bundle alongside matched binaries, with its exact hash referenced from package notices. Provide all codec, crate and font licenses/notices with each compiled app. System libraries/build tools are recorded separately and are not claimed to be bundled codec source. No claim of legal certification or byte-identical reproducibility across arbitrary toolchains.

## Completion checklist

- [x] Freeze verified nine-component source lock and safe acquisition/extraction.
- [x] Build Mac arm64 runtime locally; test actual required encoders,AV1 decoding and non-system imports.
- [x] Build native Windows x64 and Linux x64 runtimes in private CI with retained build/source evidence.
- [x] Collect exact selected Rust crate, NOTICE and font licenses; reject unresolved requirements.
- [x] Assemble source bundle and verify component archives/patches/recipe hashes against runtime manifests.
- [x] Package each application with source links/hashes, codec and Rust/font notices; preserve existing originals and prefs.
- [x] Pass real codec/geometry/size tests and native rendered GUI acceptance per platform; validate extracted packages.
- [x] Audit final public files for private data; enforce matching remote asset hashes as the final publication gate. The release operation records remote verification separately.

## Ownership

Source lock/acquisition, native runtime builder, and Rust/font notices are independent delegated tasks. Root owns integration, CI, source/package verification, local install and release. No public codec binaries before complete matched material and passing native evidence.

## Accepted evidence

Native application tests: macOS100, Windows78, Linux100; no failures/ignored tests. Final package CI run39513080541 passed both native hosts. Mac extracted package and manual0.5MB conversion passed. Full counts, hashes, scope and unsigned-package limits are in [current validation](../VALIDATION.md) and [the sanitized receipt](evidence/matched-2.3.1.json).
