# Lessons learned

A lesson is a durable, repeatable, non-obvious constraint, convention, or tool quirk; leave out incident history and generic advice.
Each lesson has a short imperative `##` title, the rule, **Do:** the required action, **Why:** the consequence (only when not self-evident), and **Source:** a link to the authority.

## Lessons

## Prefer the Ghidra CLI over the REA MCP wrapper for native analysis

REA's MCP wrapper re-spins a full Ghidra instance on every call and aborts at
330s; a full import+analysis of a large PE (e.g. the 30 MB `braintellect.exe`)
exceeds that and fails. Drive the host Ghidra CLI directly instead.
**Do:** use `/opt/ghidra_12.1.4_PUBLIC/support/analyzeHeadless` in the
background: (1) `mkdir` the project directory first — Ghidra will not create its
parent; (2) export `JAVA_HOME` to a JDK; (3) import once to analyze, then
re-import with `-postScript <script>.py` to reuse the cached analysis; (4) drop
`-scale` — after `-import <file>` it parses as a positional target.
**Why:** the CLI keeps the project database and reuses cached analysis, so the
decompile step is fast; each REA call re-analyzes from scratch and times out.
**Source:** BRAINtellect2 RE session, 2026-10-22.

## Force-add files under `.claude/`

The template-managed `.gitignore` ignores `.claude/`, so a new skill file is silently left out of the commit.
**Do:** stage it with `git add -f .claude/skills/<path>`; edit `.gitignore` only through the template.
**Source:** `.gitignore` ("Claude config" block), `.repo-template.json` (`.claude/skills/**` is `product`).
