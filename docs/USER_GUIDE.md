# Nova Flux user guide

## Conversion

Add files, drop them onto the window, or add a folder. Choose an output format and destination, then start the queue. Cancel individual items or the batch, retry a failed item, reveal an output in the file manager, or clear completed items. **Output settings** opens and closes the **Your output** panel.

Video output: MP4, MKV, AVI, MOV and WebM. Audio output: MP3, WAV, AAC, OGG, M4A and FLAC. The selected streams are the first non-artwork video and first audio stream; subtitles, artwork and alternate audio are outside this selected-stream contract. Broken files report their own errors. Folder scanning is bounded and does not follow symbolic links. Originals and existing output files are not overwritten. Closing the app cancels active work; queue history is session-only.

## Source-aware compression

For video, the quality cards choose distinct **maximum sizes relative to each source file**:

| Preset | Maximum output size |
| --- | --- |
| Compact | 40% of source bytes |
| Balanced | 65% of source bytes |
| High quality | 85% of source bytes |

These percentages are deliberate size goals, not perceptual-quality scores or fabricated predictions. The interface labels them as maximums. Smaller presets can reduce visible quality and audio fidelity; retaining dimensions does not retain original quality. CPU encoding uses two-pass bitrate allocation; compatible hardware encoding uses measured feedback and software fallback. The completed file is measured before publication and must fit the applied maximum. Exact output bytes and reduction are displayed afterwards. A very small or already minimal source may not fit a requested preset; that fails clearly and publishes no oversized or resized file.

**Video dimensions are locked to Original.** Compression does not resize, pad, lower frame rate or shorten the selected streams. Orientation and pixel aspect ratio are verified where known. Unsupported geometry/rotation combinations fail clearly rather than silently changing the image. Old saved resize settings reset to Original.

**Stream copy** remuxes compatible selected streams without re-encoding. Its size is measured through one complete private copy, with no seek, trim or extrapolation. It requires temporary disk space and has a cancellable 15-second measurement deadline. A failed measurement shows Unavailable with the reason. Container compatibility still applies.

Audio size estimates use real local encodes with the production profile. Longer re-encodes sample three timeline positions; short sources are measured whole. Sample extrapolation is approximate, and the tooltip records the measurement method. Missing measurements never become invented numbers or partial batch totals. WAV preserves decoded PCM precision; FLAC uses integer lossless compression and cannot recover quality already lost in a compressed source. These lossless formats can be larger than the input.

Input sizes use decimal MB: **1 MB = 1,000,000 bytes**. Exact source/output bytes are also displayed, and MiB is distinguished in the input tooltip. Changed queued sources invalidate their metadata and measurements; sources are checked again before conversion and publication.

## Exact size maximum

Enable **Target file size** to drag the logarithmic slider or enter a per-output maximum in decimal MB. This explicit maximum overrides the source-relative presets. Quality is calculated automatically; the same cap is not repeated as a pretend prediction under every quality option.

A successful output is at or below its maximum. The app does not pad it to an exact byte count. Original dimensions, cadence and full selected duration remain fixed. If the codec cannot fit, no output is saved. WAV, FLAC and Stream copy do not offer a lossy target-size mode.

## Performance

Automatic mode budgets worker count and CPU threads against the workload, processor count and available memory. On supported Apple encoders it compares real short CPU and hardware trials, caches the measured choice and falls back to software where needed. Other platforms currently use compatible software encoding; no untested universal GPU acceleration is claimed. RAM is used for media work, rather than allocated merely to increase its usage number.

Exact size control has a cost: CPU video uses two passes. Automatic tuning compares implemented paths; it is not proof of the fastest possible algorithm on every future device. Measurements and source probes are cancellable and stay away from the drawing thread.


## Getting help

See [Support](../SUPPORT.md) for troubleshooting and [report a bug](https://github.com/mohamadrezamakanani-dotcom/nova-flux/issues/new?template=bug_report.yml). Never attach private media or unredacted diagnostics. Security reports belong in the [private reporting channel](https://github.com/mohamadrezamakanani-dotcom/nova-flux/security/advisories/new).
