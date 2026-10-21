# Manual native acceptance

Use this checklist for a specific package, computer, and session. Record actual
observations; completing a checklist on one host does not establish every OS,
driver, codec input, or accessibility tool. The authoritative recorded release
results are in [VALIDATION.md](../VALIDATION.md) and
[CROSS_PLATFORM.md](CROSS_PLATFORM.md).

## Recorded 2.3.1 evidence

| Check | macOS arm64 | Windows x86_64 | Linux x86_64 |
| --- | --- | --- | --- |
| Native automated Rust/media suite | PASS: 100 | PASS: 78 | PASS: 100 |
| Extracted package diagnosis and framebuffer rendering | PASS | PASS | PASS |
| Native picker and actual target-size UI conversion | PASS | NOT_RUN | NOT_RUN |
| About & licenses and file-manager license navigation | PASS | NOT_RUN | NOT_RUN |
| Native folder import and destination selection | PASS | NOT_RUN | NOT_RUN |
| Drag/drop import | NOT_RUN | NOT_RUN | NOT_RUN |
| Output settings panel closes and reopens | PASS | NOT_RUN | NOT_RUN |
| Impossible target reports an error without publishing output | PASS | NOT_RUN | NOT_RUN |
| Retry failed items and complete a two-file target-size batch | PASS | NOT_RUN | NOT_RUN |
| Existing destination and source bytes remain unchanged | PASS | NOT_RUN | NOT_RUN |
| Reveal output in file manager and clear completed entries | PASS | NOT_RUN | NOT_RUN |
| Queue cancellation during work | NOT_RUN | NOT_RUN | NOT_RUN |
| Close during active work | NOT_RUN | NOT_RUN | NOT_RUN |
| Restore user preferences and verify them after relaunch | PASS | NOT_RUN | NOT_RUN |
| Keyboard-only acceptance | NOT_RUN | NOT_RUN | NOT_RUN |
| Screen-reader acceptance | NOT_RUN | NOT_RUN | NOT_RUN |

The Mac evidence is from macOS 27.0.1 / Apple M4 / Metal. Windows evidence is
Windows Server 2022 / DX12 WARP; Linux is Ubuntu 22.04 / X11-Xvfb / Vulkan
llvmpipe. Physical Windows/Linux manual desktop acceptance remains **NOT_RUN**.
Automated media tests cover more engine behavior than this manual table, but
they do not count as direct user interaction. A missing manual result is not an
automated failure.

Additional physical Mac UI checks on 2026-10-21 used two generated 3,653,251-byte
fixtures. A 1,000-byte limit failed clearly without publishing output. Retrying
both files with a 500,000-byte limit produced two 487,714-byte outputs; independent
full decoding confirmed 1920×1080, 30/1 fps, 90 frames and 3.008 seconds. The
existing destination sentinel was preserved using a `_2` filename suffix, and
both source hashes were unchanged. **Show file**, **Clear completed**, the output
panel toggle, and exact preference restoration followed by relaunch were observed.
See the [sanitized Mac session receipt](evidence/manual-macos-2.3.1-2026-10-21.json).
Drag/drop, cancellation, closing during active work, keyboard-only operation and
screen-reader use were not performed in that session and remain **NOT_RUN**.

Use `PASS` for an observed expected result, `FAIL` for a reproduced mismatch,
`PARTIAL` for an incomplete check, `NOT_RUN` for an unperformed check, and
`UNAVAILABLE` when a required device/service is unavailable. Explain any status
other than `PASS`; do not count it as passing.

## Record the session

Record the date, app version and package filename/SHA-256, source revision if
known, OS/version, processor architecture, GPU/driver, display scale, and Linux
desktop/portal where applicable. State whether it is a physical desktop, VM,
remote session, or headless CI adapter. Note any runtime overrides and whether
settings were new or existing. Keep personal paths and media out of public notes.

## Checks to perform

1. **Package and launch.** Verify the published checksum, extract into a fresh
   folder, and keep the full package together. Launch using normal desktop
   controls. Record security prompts accurately: macOS is not notarized and
   Windows is unsigned. Run `--diagnose` and record readiness; this alone does
   not prove rendering or conversion.
2. **Import and navigation.** Use the native picker with a generated or otherwise
   non-sensitive file; try folder selection and drag/drop separately. Check
   readable labels at the tested display scale, opening/closing **Your output**,
   destination selection, keyboard focus, and the dialogs actually used.
3. **Known conversion.** Choose a supported lossy format and a feasible maximum.
   Record exact source bytes, dimensions, frame cadence, selected duration, target
   bytes, mode, and final output bytes. Start through the UI. Check that the
   displayed result agrees with the file, fully decode the output, and compare
   geometry/cadence/full selected duration with the input. Confirm the original
   hash is unchanged. For 0.5 decimal MB, the ceiling is 500,000 bytes; output can
   be smaller. A new fixture need not reproduce the historical 486,199-byte result.
4. **Presets and invalid limits.** Check distinct 40/65/85% source-relative video
   ceilings. Try an impossible maximum on a disposable fixture and verify a clear
   error with no oversized, resized, or shortened published file. WAV, FLAC, and
   Stream copy do not expose a lossy target-size mode. Unsupported stream/container
   combinations should report a useful error.
5. **Queue and file safety.** With disposable fixtures, cancel an item and batch,
   retry a failure, reveal an output, and clear completed entries. Close while
   work is active and check process termination and temporary-file cleanup.
   Repeat with a pre-existing destination filename and verify its bytes remain
   unchanged. Record whether unrelated queued items continue appropriately.
6. **Source changes and restart.** Change a disposable queued source and verify
   stale metadata/measurements are invalidated. Restart and check preferences
   that should persist. Queue history is session-only. Restore any backed-up
   user preferences after testing.
7. **Licenses and desktop integration.** Open **About & licenses**, follow the
   bundled notice location, and verify file-manager navigation. Test duplicate
   launch behavior on the actual OS. Record dialog/focus behavior separately
   from the conversion result.
8. **Accessibility.** Test keyboard-only operation and the named native assistive
   technology, including import, settings, start/cancel, progress/errors, and
   dialogs. Record what is reachable and announced. Native UI/toolkit support
   alone does not establish an accessible workflow.

Retain a minimal sanitized result table and relevant captures. File public
regressions through the [bug form](https://github.com/mohamadrezamakanani-dotcom/nova-flux/issues/new?template=bug_report.yml);
use [private reporting](../SECURITY.md) for suspected vulnerabilities. Never
attach private media, credentials, or complete raw configure/environment logs.
