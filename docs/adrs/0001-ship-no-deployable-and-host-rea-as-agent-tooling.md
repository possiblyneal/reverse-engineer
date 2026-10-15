---
type: Architecture Decision Record
title: Ship No Deployable and Host REA as Project-Scoped Agent Tooling
description: The repository holds no unit under apps/; it exists to scope REA's MCP server and skill to sessions opened here.
scope: [global]
tags: [agent-tooling, reverse-engineering]
sources:
  - id: rea
    resource: https://github.com/morluto/rea
    title: "REA: Reverse Engineer Anything"
generated: { by: "agent/claude-opus-5-5", at: "2026-10-15T06:15:00Z" }
superseded_by:
status: accepted
---

# Ship No Deployable and Host REA as Project-Scoped Agent Tooling

## Decision

`apps/` holds no unit at generation. The repository is a workspace whose purpose is to register REA (the `rea-agents` npm package: an MCP server, a CLI, and a workflow skill) at project scope, so its large tool catalog and instructions load only in sessions opened in this repository. Ghidra headless, a JDK, and the other analysis engines REA drives are host installs, not repository content.

## Context

Established at generation from the short-form wayfinding questions. Asked what ships separately, the operator answered "possibly none": nothing here is delivered on its own. Asked what binds the language, the answer was whatever interfaces with Ghidra headless, MCP servers, and skills. All three are supplied by REA itself, which is a Node.js package run through `npx` or a global install, so no in-repository language is selected.

The binding constraint is context cost: REA registered at user scope would add its MCP tools and skill to every session on the machine.

## Alternatives Considered

- **A unit wrapping REA (`apps/rea/` with a `package.json` pinning `rea-agents`).** Rejected while nothing here is built or shipped: a pinned manifest makes the Node adapter require six package scripts that would check nothing of this repository's own. Revisit when this repository first carries its own code, such as Ghidra scripts or a bridge of its own.
- **Registering REA at user scope.** Rejected: it is the context cost this repository exists to avoid.

## Consequences

`scripts/check` reports no project manifest until one is added. REA's MCP registration lands in `.mcp.json`, which the template manages, so a template update reconciles its change with REA's entry rather than overwriting it. The REA skill under `.claude/skills/` is product-owned and never touched by an update.
