# Security Policy

## Report a vulnerability privately

Use GitHub's [Report a vulnerability](https://github.com/mohamadrezamakanani-dotcom/nova-flux/security/advisories/new)
route. If that form is unavailable, check the repository's
[Security page](https://github.com/mohamadrezamakanani-dotcom/nova-flux/security)
for reporting availability. Do not put vulnerability details in public issues or
pull requests. There is no guaranteed response or remediation timeline.

Include the affected version/commit and package, OS/architecture, minimal
reproduction steps, expected boundary, and observed impact. Prefer a generated,
non-sensitive sample. Never include credentials, private media, personal paths,
or complete raw environment/build logs. Share the smallest necessary evidence
through the private report.

Security investigation focuses on the latest release and current source. Older
releases have no separate maintenance commitment. Review
[release validation](VALIDATION.md) for actual tested scope; passing checks are
not a security certification.

## System and trust boundaries

Nova Flux is a local Rust desktop application. It probes and converts selected
local files using separate native FFmpeg/ffprobe processes. The application,
media runtime, and development tools execute with the current user's permissions;
Nova Flux is not an OS sandbox for hostile media. Treat imported media and its
metadata as potentially malicious input. The chosen runtime executables and
development override paths are trusted code.

Assets include input files, existing destination files, local preferences,
temporary media, and release/source inventories. File paths, media metadata,
folder contents, and changing files cross application/process boundaries.
Archive entries, notices, manifests, and upstream source downloads also cross
build/packaging boundaries. The matched release runtime disables media networking;
the installed app needs no media upload or runtime download.

## Properties to protect

- Originals and existing outputs must not be overwritten. Invalid, oversized,
  or cancelled work must not be published as a completed output.
- Subprocess arguments and file handling must not turn filenames or metadata into
  shell execution, unintended path traversal, or access outside the intended work.
- Probing, folder scanning, subprocess lifetime, and temporary work must retain
  their bounds and cancellation behavior. Changed sources must invalidate stale
  metadata/measurements and be rechecked before publication.
- Package extraction must reject unsafe entries. Runtime, retained source,
  notices, and final inventories must remain bound to their verified inputs.
- Public packages and reports must not expose secrets, private media, personal
  paths, or raw shell-environment dumps. Original source and license notices must
  remain intact.

## Reportable impact and limitations

Report a reachable violation of these boundaries, such as unintended code
execution, disclosure, destructive file handling, unsafe extraction, or a bypass
of a resource or integrity control. Explain the required user action and actual
impact; an isolated suspicious code pattern is not proof of exploitability.
Upstream codec issues affecting the shipped runtime are relevant, with the exact
version and reachable behavior identified.

Ordinary format failures, visual quality, and feature requests normally belong in
[support](SUPPORT.md). A crash with a plausible security impact should be reported
privately. Control of a user's runtime executable is already local code execution;
it does not establish a separate Nova Flux exploit without another boundary
violation. This policy introduces no blanket exclusion for malformed local media
and records no accepted vulnerability. Unverified controls must not be treated
as proof that a finding is safe.
