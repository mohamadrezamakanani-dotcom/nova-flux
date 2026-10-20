# Support

Start with the [README](README.md), the [current validation record](VALIDATION.md),
and [platform scope](docs/CROSS_PLATFORM.md). Nova Flux runs locally; no account
or media upload is required. Community support is handled through repository
issues, without a guaranteed response time.

## Before opening an issue

1. Confirm the application version and download the package for your OS and
   processor from [Releases](https://github.com/mohamadrezamakanani-dotcom/nova-flux/releases).
   Keep the complete extracted package together. The source ZIP alone is not an
   application or a prebuilt media runtime.
2. Check the reported error. A very small byte maximum, unsupported stream/container
   combination, or unsupported geometry can fail deliberately. Lower size caps
   can reduce quality; keeping dimensions does not preserve original quality.
   A maximum is a ceiling, so an output may be smaller than requested.
3. Retry with a small generated or non-sensitive file you can legally share, a
   writable local destination, and enough free space for temporary media work.
   Avoid changing an input while it is queued.
4. Search [existing issues](https://github.com/mohamadrezamakanani-dotcom/nova-flux/issues).
   Add useful reproduction details to a matching report or open a
   [bug report](https://github.com/mohamadrezamakanani-dotcom/nova-flux/issues/new?template=bug_report.yml).

macOS packages are ad-hoc signed and not Apple-notarized; Windows executables
are unsigned. Verify the release and published checksum before deciding whether
to open a package. For missing-runtime errors, re-extract the complete matching
package. Source builds need the native runtime described in
[CONTRIBUTING.md](CONTRIBUTING.md); installed packages do not need `NOVA_RUNTIME`.

## Useful, safe diagnostic information

Report the version (or source commit), package filename, OS version, architecture,
and, for rendering problems, GPU/driver or CI adapter. Include numbered steps,
expected and actual behavior, and the exact error text after sanitizing it.

For a media problem, record input/output format and codecs if known, dimensions,
frame rate, duration, **exact input/output bytes**, selected preset or target byte
maximum, and performance mode. Decimal **1 MB = 1,000,000 bytes**. State whether
the problem repeats with a generated sample. For source builds also provide Rust
version, runtime source/build identity, and the relevant command and exit status.

The `--diagnose` switch prints runtime readiness and versions; it does not run a
conversion or prove GUI support. From the extracted package's containing folder:

```sh
# macOS
"./Nova Flux.app/Contents/MacOS/nova-flux" --diagnose
# Linux, from the extracted application folder
./nova-flux --diagnose
```

For Windows, from the extracted application folder in PowerShell:

```powershell
.\nova-flux.exe --diagnose
```

Review diagnostic text before posting. Remove personal filenames, full home
paths, usernames, email addresses, location/media metadata, tokens, and unrelated
environment values. Crop screenshots to the application and inspect attachments.
Share only the smallest relevant sanitized excerpt. Do not attach private media
or complete raw build/configure logs; FFmpeg configure diagnostics can contain
the entire shell environment. A generated reproduction is preferred.

## Security and platform limits

Report suspected vulnerabilities through [private security reporting](SECURITY.md),
not a public issue. Do not publish secrets or exploit details while asking for help.

The 2.3.1 native suites passed **100 macOS / 78 Windows / 100 Linux** tests on
the recorded hosts. Windows/Linux CI uses software graphics adapters. Manual
interaction on physical Windows/Linux computers remains **NOT_RUN** in the
release evidence. Use [MANUAL_ACCEPTANCE.md](docs/MANUAL_ACCEPTANCE.md) to record
new platform results without treating them as support for every device.
