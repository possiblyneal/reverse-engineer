---
type: Architecture Decision Record
title: Render Recorded Tones from a Freely Licensed SoundFont
description: The clinician can pick harp, acoustic guitar or xylophone as the feedback tone; the clone renders them once from a MIT-licensed General MIDI SoundFont the clinician supplies, and carries no sample bytes in Git.
scope: [apps/braintellect-linux]
tags: [audio, licensing, ui]
generated: { by: "agent/claude-opus-5-5", at: "2026-11-07T00:00:00Z" }
superseded_by:
status: accepted
---

# Render Recorded Tones from a Freely Licensed SoundFont

## Decision

The set-up screen lists the feedback tones: the original banks 1002 (default, "Acoustic strings"), 1001 ("Piano") and 1003 ("Celeste"), then recorded Harp, Acoustic guitar and Xylophone. Note decisions do not change; only the sample set that sounds them does. `braintellect-linux import-soundfont <file.sf2>` renders each recorded tone once, for MIDI notes 40 to 76 (the clamp in `src/feedback/note.rs`), resampled to the 96 kHz sink rate and 0.8 s long, into the instruments directory of ADR 0004 as `tone-<key>.bank`. A tone that was never imported is listed as unavailable with the command that imports it; it does not fail a session. No sample bytes are committed; tests use a synthetic SoundFont.

## Context

The operator asked for harp, guitar and xylophone alongside the original sounds (ticket 65). The tones must be recordings under a licence that allows use and redistribution. The recorded source is FluidR3_GM (General MIDI programs 46 Orchestral Harp, 24 Nylon Guitar, 13 Xylophone), MIT licensed, from Debian's `fluid-soundfont-gm` package; the URL, checksum and licence are in `docs/reverse-engineering/braintellect2/audio-feedback.md`, "Recorded tones".

## Alternatives Considered

- **Commit the rendered tones.** Rejected: the same size and provenance reasons as ADR 0004.
- **Synthesise the tones.** Rejected: the operator asked for recorded instruments.
- **Add a SoundFont crate.** Rejected: reading the few SF2 tables the renderer needs is about 250 lines, and no SF2 crate is already installed.

## Consequences

- The clinician needs an `.sf2` once (the MIT-licensed FluidR3_GM, or any GM font with programs 13, 24 and 46 covering notes 40 to 76); a font that lacks one is refused and nothing is written.
- Rendering supports only what these presets use: sample, key range, root key, tuning, loop mode, and left mono samples. Envelopes, filters and modulators are ignored.
