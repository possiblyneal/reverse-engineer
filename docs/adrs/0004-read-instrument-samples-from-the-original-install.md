---
type: Architecture Decision Record
title: Read Instrument Samples from the Clinician's Original Install
description: The clone plays the original's recorded instrument samples, imported once from the clinician's BRAINtellect2 install (lib/dll*.BT.dll), and never carries those samples in Git.
scope: [apps/braintellect-linux]
tags: [audio, reverse-engineering, parity, licensing]
generated: { by: "agent/claude-opus-5-5", at: "2026-11-06T00:00:00Z" }
superseded_by:
status: accepted
---

# Read Instrument Samples from the Clinician's Original Install

## Decision

The clone's feedback sound is the original's own recorded instrument samples. A one-time
import copies every instrument bank (`lib/dll<id>.BT.dll`, e.g. 1001, 1002, 1003) from the
clinician's BRAINtellect2 install into the clone's data directory, and the audio layer reads
the bank a design names (SMART-2 uses 1002) from there. A missing bank is an error the
clinician sees, never a silent fallback. The samples are never committed to Git; tests use
synthetic banks.

## Context

Static analysis (`docs/reverse-engineering/braintellect2/audio-feedback.md`, "Wave device and
instrument 1002") showed that the original's "Piano" is 45 recorded 96 kHz samples inside
`dll1002.BT.dll`, played as is; `braintellect.exe` holds no waveform to recover. The
operator chose identical sound over independence from the original install (2026-11-06),
and asked that every tone bank be imported, not only 1002.

## Alternatives Considered

- **Commit the samples to Git.** Rejected: they are the vendor's recorded content, and at
  about 7 MB per bank they exceed the 500 KB fixture limit.
- **Synthesise an approximation.** Rejected: it sounds different from the tablet and cannot
  be checked against the recorded feedback audio.

## Consequences

- The clinician needs the original install's `lib` directory once, before the first session.
- The clone depends on the vendor's bank format (RIFF/WAVE blocks plus the DLL's note map);
  it parses the file, it never loads or runs the DLL.
