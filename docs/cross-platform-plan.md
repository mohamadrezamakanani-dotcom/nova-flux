# Cross-platform compression release plan

Scope approved in conversation: correct source-aware compression presets at original video dimensions; prepare and test Windows and Linux before GitHub publication. Preserve local preferences, input media and the original Nova Converter app. English native Rust UI; bundled external FFmpeg remains native codec code.

- Source-relative video ceilings: Compact40%, Balanced65%, High85%, enforced on completed files by the measured budget encoder. These are explicit size goals, not fabricated predictions or universal perceptual-quality scores. Stream copy and lossless audio remain separate.
- Platforms: macOS arm64 already validated; add native Windows x86_64 and Linux x86_64, each with platform paths, executable naming, desktop file reveal, available-memory scheduling and separate portable runtime/package. Rust itself does not make one binary run everywhere.
- Validation: existing library/engine/size/geometry/source/cancel tests plus new preset/portable tests; real codecs on each native OS; native WGPU window screenshot and graceful smoke shutdown. GUI smoke is narrower than manual interaction on physical PCs and GPUs.
- Stage CI in a private GitHub repository with standard hosted Windows/Linux runners. No public publication before results. Keep third-party runtime validation artifacts private; corresponding-source completeness is unresolved for the supplied runtime builds.
- Public path: audited application source, tests, scripts, assets/provenance and accurate support table. Exclude personal media/preferences, build caches and media runtime binaries. Choose source license separately. Public self-contained binary redistribution requires complete matching runtime sources and notices; code availability is not proof of that work.
