## Template Contract

These sections are identical in every repository built from the template. Do not edit them here: a repository whose shape needs to differ has found a gap in the template, so fix it there and let the next template update (`/repo-builder`) carry it. This repository's own instructions start at Purpose.

### Commands

Use these instead of per-language tools. `check` and `fix` run for every language present and report `pass`, `not-applicable`, `unavailable`, or `FAIL`.

- `scripts/check` — the full local gate, a superset of CI except the pull request's changelog check: doctor, script tests, structure, lint, format, type check, test, build, security audit, pre-commit over every file. Prints only the Result table, with the full run in `tmp/check.log`; `--verbose` streams everything
- `scripts/fix` — lint autofix, then formatting
- `scripts/summarize <command…>` — run a check and print only its Result table; read `ci` and other commands through this, since `check` already does
- `scripts/run [unit] [--entry <name>] [-- args…]` — start a unit; the unit name is required when `apps/` holds several

`ls scripts/` lists the rest, such as `doctor`, `integration`, and `package`.

### Layout

`scripts/check` fails on any of these; pre-commit runs `scripts/structure` over the placement rules on every commit.

- Root holds only `apps/`, `docs/`, `libs/`, `scripts/`, `tests/`, `tools/`, `deploy/`, `assets/`, `gradle/`, `tmp/`, and the dot-folders `.claude/`, `.github/`, `.devcontainer/`.
- Root files are permitted by name: the shipped dotfiles, `CLAUDE.md`, `GLOSSARY.md`, `GLOSSARY-MAP.md`, `.gitmodules`, `.structure-allow`, `.repo-template.json`, adopted addons, and each language's root manifests and lockfiles. Any other root file or folder needs an entry in `.structure-allow`.
- `apps/<unit>/` is one unit: the smallest piece delivered on its own. Its code goes in `apps/<unit>/src/`. Nothing sits directly in `apps/`.
- Split a unit into domains only for distinct business areas: `apps/<unit>/<domain>/src/`. A unit has `src/` or domains, never both. Domains do not nest.
- A root, unit, or domain may hold its own `libs/`, `tests/`, `scripts/`, `tools/`, `assets/`, `docs/`, or `deploy/`, scoped to it. These never contain `src/`.
- `tools/` holds one directory per program, never loose files.
- `deploy/` holds only `quadlet/`, `containerfile/`, `compose/`, `systemd/`, and `env/`.
- `docs/` takes Markdown; anything else there needs `.structure-allow`.
- Below a `src/`, the language decides the layout.
- A path ending in `/` in `.structure-allow` exempts that whole prefix, for vendored or fixture trees.
- A package whose language has no root manifest fails the gate; add the root workspace manifest (`go.work`, `settings.gradle.kts`, and so on) that lists it. Swift is excepted: it has no root manifest.

These are conventions `scripts/structure` does not check:

- Root-level `libs/`, `tests/`, `scripts/`, `tools/`, `deploy/`, and `assets/` hold only what spans several units.
- Files a program reads go in `assets/`. Files it writes while running go in `apps/<unit>/state/`, which is gitignored.
- `tests/integration/` is the integration tier, run by `scripts/integration` and left out of the gate.

Every unit has a `.unit.json` at its root, never at a domain's:

```json
{ "schema_version": 1, "run": "oneshot", "ships": { "kind": "executable", "targets": ["linux-amd64"] } }
```

- `run`: `oneshot` (exits on its own), `longlived` (runs until stopped), or `none`.
- `ships.kind`: `executable`, `quadlet`, or `none`. `ships.targets` (`linux-amd64`, `macos-arm64`) is required for `executable` and rejected otherwise.
- Any pairing of `run` and `ships` is valid.

## Purpose

Workspace for reverse-engineering sessions with REA (`rea-agents`). It scopes the REA skill and its CLI-only tooling to sessions opened here (`docs/adrs/0001-ship-no-deployable-and-host-rea-as-agent-tooling.md`; REA runs CLI-only, see Local Contracts). It has two units, and nothing here is released as a deployable: `apps/braintellect-collector/`, a non-shipping Windows tablet collector (`docs/adrs/0002-add-braintellect-collector-unit.md`), and `apps/braintellect-linux/`, the Rust reimplementation of BRAINtellect2 built, run, and tested in-repo as the parity oracle for the reverse-engineering effort (`docs/adrs/0003-host-linux-clone-as-run-and-test-unit.md`).

## Ownership

## Local Contracts

- REA runs **CLI-only**, invoked as `npx -y rea-agents@6.8.0 <command>` pinned to an exact version. It is **not** registered as an MCP server: there is no `.mcp.json`, and the REA/MCP Ghidra provider is not used (it re-analyzes the 30 MB exe per call under a ~330 s deadline and times out). Analyze native targets with `analyzeHeadless` and the `rea-agents` CLI subcommands.
- `.claude/skills/reverse-engineer-anything/` is the skill copied verbatim from the same `rea-agents` tarball (currently version 49 / `tool_count` 229, from 6.8.0). Bump the skill and the pinned CLI version together from one tarball, never one alone.
- Ghidra, a JDK, and other analysis engines are host installs, not repository content.

## Work Guidance

## Verification

## Child Index

- `apps/braintellect-collector/CLAUDE.md` — the portable BRAINtellect2 collector, its owner delivery archive, and its VM test.
- `apps/braintellect-linux/CLAUDE.md` — the Rust BRAINtellect2 clone, its parity contract, and its layer plan.
- `docs/reverse-engineering/CLAUDE.md` — durable, non-sensitive reverse-engineering findings.

`scripts/` and `.github/` are template-managed.
