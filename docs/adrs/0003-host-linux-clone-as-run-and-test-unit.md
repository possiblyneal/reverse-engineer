---
type: Architecture Decision Record
title: Host the BRAINtellect2 Linux Clone as a Run-and-Test Unit
description: The Rust reimplementation of BRAINtellect2 lives in apps/braintellect-linux/, built, run, and tested in-repo as the parity oracle for the reverse-engineering effort; it ships no release artifact.
scope: [apps/braintellect-linux]
tags: [repository-structure, reverse-engineering, parity, lang:rust]
generated: { by: "agent/claude-opus-4-8", at: "2026-11-03T00:00:00Z" }
superseded_by:
status: accepted
---

# Host the BRAINtellect2 Linux Clone as a Run-and-Test Unit

## Decision

`apps/braintellect-linux/` is the repository's second unit and its first that
carries its own source and runs itself: a from-scratch Rust reimplementation of the
BRAINtellect2 neurofeedback application for Debian Linux, driving the B-2 EEG
controller over Bluetooth RFCOMM. Its `.unit.json` declares `run: longlived` (the
session UI runs until the clinician stops it) and `ships: { kind: none }`: it is
built and run in-repo to validate behavioral parity against the captured real-app
baseline, not released as a deployable. Rust is the repository's first selected
language, so a root Cargo workspace manifest appears.

This does **not** supersede ADR 0001. The repository remains a reverse-engineering
workspace that hosts REA, and "ship no deployable" holds literally — the clone ships
nothing. It extends ADR 0002's non-shipping-unit precedent to a unit that *runs* and
carries code, which is the "first code of its own / a bridge of its own" revisit
point ADR 0001 names.

## Context

The clinician's BRAINtellect2 runs on failing NuVision tablet hardware (32-bit Qt,
embedded MySQL, a Windows-only Bluetooth stack, and per-host license binding) that
cannot be reinstalled or moved as-is. The developer needs the same neurofeedback
experience on Debian Linux, driving the B-2 directly, so the clinician no longer
depends on unsupported Windows hardware.

The developer's decision (2026-11-03) is that building, running, and testing the
clone is itself this workspace's reverse-engineering work: the clone is validated
only by diffing its observable behavior against a captured real-app baseline, which
requires running it here, and this repository owns its ongoing maintenance and
features. Hosting it in a separate repository would detach the clone from the
evidence that defines its correctness — the captured baseline, the protocol spec,
and the REA analysis — all of which live under this workspace.

## Alternatives Considered

- **A new dedicated repository for the clone.** Rejected: parity against a captured
  baseline is the correctness contract, and the baseline artifacts, the B-2 protocol
  spec, and the REA analysis all live here. A separate repository would force every
  parity check to straddle two repositories and separate the clone from the evidence
  that validates it.
- **Declare `ships: executable`.** Rejected: nothing here is released on its own
  (ADR 0001). The clone is run in-place for parity validation; `scripts/package`
  would otherwise demand release binaries for `linux-amd64`/`macos-arm64` that are
  not produced.

## Consequences

The repository gains its first language toolchain: `scripts/check` now runs Rust
checks (format, clippy, test, build) that must stay green, and the structure gate now
requires a root `Cargo.toml` workspace manifest. The clone's correctness depends on a
captured baseline that does not yet exist (ticket T2); acquisition-layer parity cannot
be asserted until that capture resolves the protocol spec's open items (`setStart`
framing, the 110/112/131 reply layouts, and the device family). Root `CLAUDE.md`'s
"one unit" wording is updated in this change to name both units. The workspace's charter now explicitly
includes running a reconstruction, not only analyzing the original.
