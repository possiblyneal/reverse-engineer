---
type: Architecture Decision Record
title: Add the BRAINtellect2 Collector as a Non-Shipping Unit
description: The portable BRAINtellect2 tablet collector lives in apps/braintellect-collector/, which runs nothing here and ships nothing through scripts/package.
scope: [apps/braintellect-collector]
tags: [deployment, repository-structure, windows]
generated: { by: "agent/claude-sonnet-5-5", at: "2026-10-20T16:00:00Z" }
superseded_by:
status: accepted
---

# Add the BRAINtellect2 Collector as a Non-Shipping Unit

## Decision

`apps/braintellect-collector/` is the repository's first unit. Its `.unit.json` declares `run: none` and `ships: { kind: none }`: it holds one PowerShell script and its owner instructions, which `scripts/build-delivery` zips and a person delivers to a Windows tablet by hand. It is tested through its headless Interface (`BST_NO_UI=1`) against a synthetic Windows VM fixture, never against a physical tablet. It does not supersede ADR 0001: the repository still hosts REA as agent tooling, and the collector is the "first code of its own" that ADR names as a revisit point.

## Context

A BRAINtellect2 tablet owner needs one safe script that returns the installation, database, registry keys and diagnostics from a Windows 8, 8.1 or 10 tablet. The tablet cannot be reached during development, so the script is verified in a Windows 11 VM seeded with synthetic data; that VM says nothing about the tablet's operating system. The compatibility floor is Windows 8, PowerShell 3 and .NET 4.5, which rules out `Compress-Archive`. The script reads the database only from a private temporary copy, never modifies the tablet, and transmits nothing: the owner sends the resulting ZIP.

## Alternatives Considered

- **Declare `ships: executable`.** Rejected: `scripts/package` builds binaries for `linux-amd64` and `macos-arm64`, and the deliverable is a script for Windows that is handed over manually.
- **Keep the script outside the repository.** Rejected: the owner contract and the VM test are what make its failure behaviour trustworthy.

## Consequences

The unit has no project manifest, so `scripts/check` reports its language checks not-applicable and `scripts/integration` finds nothing to dispatch to. The two tests under the unit are run by hand and are not part of the gate. The delivery archive is built on demand and is not a release asset.
