## Purpose

Durable, non-sensitive technical findings from authorized analysis of local
reverse-engineering targets. One subdirectory per target; cross-target contracts
and the target index live here.

## Ownership

- Each target owns a subdirectory `<target>/` with its own `CLAUDE.md` indexing
  that target's findings docs (and any asset subtree, e.g. `literature/`). Name
  findings docs by topic within the target dir (`b2-protocol.md`), not with a
  target prefix — the directory already names the target.
- Nothing sits directly in this directory except this `CLAUDE.md` and the target
  subdirectories.

## Local Contracts

- Do not put patient, session, account, or other personal data in this directory.
- Separate observed configuration from inferred behavior; cite the source artifact and method for every material claim.
- Constants recovered from the target artifact itself — hardcoded cipher keys, IVs, obfuscation constants, and the like — are documentable findings: record them inline in the findings doc with their extraction site (VA/file offset), since any holder of the artifact can re-extract them.
- Keep genuinely secret or personal material out of Git and under the repository `tmp/` directory: raw exports, SQL dumps, decoded session records, live credentials, and any decrypted payload carrying patient or session data.
- Run Ghidra through its CLI (`analyzeHeadless`), never through the REA/MCP Ghidra provider: each MCP call re-imports and re-analyzes the 30 MB `braintellect.exe` under a ~330 s deadline and times out. Import and analyze once with `analyzeHeadless` into a persistent project, then query with headless post-scripts.

## Work Guidance

- Record only reproducible technical findings that remain useful beyond the active session.
- Preserve important uncertainty explicitly; do not convert an unverified implementation detail into a fact.

## Verification

- Review reports for accidental sensitive-data disclosure before committing or sharing.

## Child Index

- `braintellect2/CLAUDE.md` — BRAINtellect2 / B-2 findings (wire protocol, device
  connection, audio feedback, session storage) and the HIRREM literature corpus.
