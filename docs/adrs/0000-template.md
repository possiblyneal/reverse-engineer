---
type: Template                # replace with: Architecture Decision Record
title: <The Decision as One Imperative Statement, e.g. "Use PostgreSQL for the Primary Datastore">
description: <Write the decision as one sentence. Scans and indexes surface this without opening the file.>
scope: [] # What the decision governs: `apps/<app-name>` for one deployable, `domain` for a business area rather than a deployable, `global` for the repository as a whole
tags: [] # How the decision is found: high level architectural themes only (Max 5), plus `lang:<name>` where one language is the subject
# sources:
#   - id: <stable-source-id>
#     resource: <URL or bundle-relative path>
#     title: <Human-readable source title>
generated: { by: "<actor, e.g. human:neal or agent/model>", at: "<ISO 8601 datetime, e.g. 2026-08-05T14:30:00Z>" } # update after each meaningful change
# verified: { by: "<actor>", at: "<ISO 8601 datetime>" } # uncomment after confirming that this ADR accurately records the decision, context, alternatives, and consequences (if necessary) against its cited sources or other authoritative records; use a list for several
# supersedes:                 # path to the ADR this replaces (if and when applicable)
superseded_by:                # path to the replacing ADR; set status: superseded when filling this
status: proposed              # proposed | accepted | deprecated | superseded
---
<Copy this file to NNNN-slug.md — number: scan for the highest existing
number and add one; slug: the title compressed to a few kebab-case words,
e.g. 0001-use-postgresql.md.
Replace every frontmatter line and every placeholder, then delete this note.>

# <Decision title, restated>

## Decision

<The full statement in one to three sentences, with scope: what this
covers and what it explicitly does not. The frontmatter description is
the one-line form; this is the precise form.>

## Context

<What forced the decision: the constraints, requirements, deadlines,
team skills, and pressures in play at the moment of commitment. This is
the section most often skipped and the most valuable — it is what lets
a future reader judge whether the decision still holds once conditions
have changed.>

## Alternatives Considered

<Optional — delete this heading and this note together when the section
would add nothing. Most ADRs won't need it.

One entry per option, each with why it was rejected. State the
rejection reason precisely enough that a reader can tell when it no
longer applies. If a reader can still ask "but why didn't we just…?",
this section has failed.>

## Consequences

<Optional — delete this heading and this note together when the section
would add nothing. Most ADRs won't need it.

What we accept as a result — good and bad. If only upsides are listed
this is a sales pitch, not a record; the honest costs are the payload.>
