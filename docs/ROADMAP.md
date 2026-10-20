# Roadmap

Nova Flux's current baseline is the [2.3.1 release](../CHANGELOG.md): a local native
converter with source-relative/explicit size maxima and preserved video geometry,
plus matched-source packages for the three validated native targets. This is a
working priority list, not a delivery schedule or a guarantee that every proposal
will ship.

## Next validation priorities

- Record manual acceptance on physical Windows and Linux desktops, including file
  and folder dialogs, drag/drop, conversion, cancellation, output reveal, and
  license navigation. These checks are currently **NOT_RUN** in the release
  evidence. Use the [manual matrix](MANUAL_ACCEPTANCE.md).
- Gather reproducible compatibility reports for specific OS releases, desktop
  portals, displays, and graphics drivers. Add a supported environment only after
  evidence; the existing CI hosts do not establish universal compatibility.
- Exercise keyboard and assistive-technology workflows on native desktops and
  document observed gaps before making accessibility claims.
- Evaluate signed Windows distribution and Apple notarization, including their
  cost and release requirements. Current Windows packages are unsigned; macOS
  uses an ad-hoc signature.

## Ongoing maintenance

Keep contributor checks, source/runtime/package binding, dependency notices,
privacy controls, and release evidence reproducible. Fix confirmed regressions
with focused tests. A changed frozen codec recipe requires fresh source bundles,
native rebuilds, and acceptance on all three targets.

Potential new encoder paths or architectures need a concrete user need, native
hardware, measurements, and the same size/geometry invariants before adoption.
NVENC, Quick Sync, VAAPI, Intel Mac, Windows/Linux ARM64, and mobile support are
not established by the current release. Automatic selection is measured among
implemented paths; there is no claim of a universally fastest converter.

Use a [feature request](https://github.com/mohamadrezamakanani-dotcom/nova-flux/issues/new?template=feature_request.yml)
to describe a problem, a concrete example, and acceptable tradeoffs. Milestones
and dates should reflect work that has actually been agreed and scoped.
