# Demonstration provenance

[View the demonstration](../assets/demo.gif). Created on 2026-10-21 from the
recorded Nova Flux 2.3.1 native macOS interface and generated acceptance media.
The title in the asset is **Recorded interface + verified conversion sample**.

## What the demo shows

- **0–3 seconds:** the actual [native macOS screenshot](../assets/screenshots/macos.png),
  scaled proportionally and shown in full. The nearby workflow text is an
  explanatory overlay. This is a recorded interface image, not a live UI recording;
  no clicks or conversion progress are reconstructed.
- **3–12 seconds:** synchronized side-by-side playback of the generated original
  and the actual application-produced output. The same 90-frame, three-second
  video sample plays three times at normal source speed for viewing. The footage
  is scaled equally without cropping or color treatment. Audio is muted.
- The applied maximum was **500,000 bytes** (0.5 decimal MB). The original file
  was **3,653,251 bytes** and the actual app output was **486,199 bytes**. Both
  original video streams have **1920×1080, 30 fps, and 90 frames**. Container
  durations were 3.021 seconds and 3.008 seconds respectively, including audio
  padding. The exact native conversion is recorded in [VALIDATION.md](../VALIDATION.md).

The clip demonstrates this one already completed conversion. It does not show
encoding elapsed time, establish a speed benchmark, or imply every source has
this size or quality result. GIF scaling, palette quantization, and reduced demo
frame rate also make it unsuitable for judging pixel-level output fidelity.

## Source identity

The generated original and output were taken from the 2.3.1 native UI acceptance
session, whose `manual-ui-check.json` receipt reports a successful conversion,
full decode, unchanged source hash, and the same byte/geometry values above.
No personal media or private user filenames were used. Original media and the
receipt were retained unchanged; hashes were checked again after rendering.

| Source | Bytes | SHA-256 |
| --- | ---: | --- |
| Native interface PNG | 454,597 | `bba1213c395652d19bde76de1de67d8878192389fe5e206ce42df93e23a675ab` |
| Generated input MKV | 3,653,251 | `cac0da94a9c73171d02f94464837e24d8bbf9f5b498a474cd238de7a4afc9df5` |
| Actual app output MP4 | 486,199 | `137a70b62642c78e00eaa085d7aa40de5214e08335e5491d2f0ab316e1ace9ed` |

## Composition and verification

The composition was authored and rendered locally with **HyperFrames 0.8.181**
(the new local project pin was upgraded from scaffold version 0.8.111), using
Chrome on Apple M4 with Metal-backed hardware graphics. It contains two mounted
scenes, a paused deterministic timeline, and framework-owned video playback.
GSAP 3.14.2 and the Montserrat/IBM Plex Mono fonts were embedded at compilation.
No authenticated media provider or generative service was used.

`hyperframes check --strict` passed: **0 lint/runtime/layout/motion errors or
warnings**, 9 layout samples, and **51/51** text contrast checks. Representative
composition snapshots and frames decoded from the final GIF were visually
inspected. The renderer extracted all 90 source video frames for each sample
placement; sparse-keyframe notices did not require changing the original clips.
The 30 fps rendered master fully decoded and contained 360 frames over 12 seconds,
with no audio stream.

The final GIF was encoded from that rendered master using the same local
FFmpeg 8.1.3 runtime as the project. Nominal sampling is 12 fps; GIF stores
centisecond delays, so individual frame delays are quantized. All **144 frames**
decoded, their total duration is **12.00 seconds**, dimensions are **1024×576**,
and the GIF loops indefinitely. Frame inspection included the opening and each
sample repetition. The original media hashes remained unchanged.

Final GIF: **3,573,513 bytes**, SHA-256
`991aa13afbc145c9e8a8c6e45a4f5ef45411e8d9b2ea1652dd007ae3e949c148`.

The local encode used:

```sh
ffmpeg -i nova-flux-demo.mp4 \
  -filter_complex '[0:v]fps=12,split[a][b];[a]palettegen=stats_mode=full:reserve_transparent=0[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle' \
  -an -loop 0 demo.gif
```

The final asset is a presentation derivative. It does not replace the native
application tests, source/runtime/package receipts, or outstanding physical
Windows/Linux [manual acceptance](MANUAL_ACCEPTANCE.md).
