---
type: Architecture Decision Record
title: Build the Session UI as an egui Desktop Window
description: The clone's clinician-facing session UI is a single egui (eframe) desktop window in the clone's own binary, not a GTK app or a browser page.
scope: [apps/braintellect-linux]
tags: [ui, lang:rust]
generated: { by: "agent/claude-opus-5-5", at: "2026-11-06T00:00:00Z" }
superseded_by:
status: accepted
---

# Build the Session UI as an egui Desktop Window

## Decision

The session UI, and every later clinician-facing screen, is an egui window (`eframe`) in the
clone's binary. UI state is kept apart from drawing so it is testable headlessly.

## Context

The screen is small (connection, sensor status, session properties, start/stop, live
feedback, saved confirmation). CI is template-managed and installs no GUI development
packages; egui builds without them. The operator chose this on 2026-11-06.

## Alternatives Considered

- **GTK 4 (gtk4-rs).** Rejected: CI would need GTK development packages through a template
  change, the bindings are more verbose, and UI tests are harder. Revisit if Debian theme
  integration or full screen-reader support becomes a requirement.
- **Browser page served by the clone.** Rejected: an extra browser step every session.

## Consequences

- The window does not follow the Debian/GNOME theme, and screen-reader support is partial.
- Replacing the toolkit later rewrites only the UI layer.
