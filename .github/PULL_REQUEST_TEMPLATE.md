<!-- Named templates live in .github/PULL_REQUEST_TEMPLATE/, and nothing in
     GitHub's interface offers them: a pull request gets this file unless the
     URL asks for one by name. The links below are what make them reachable by
     clicking -- from the Preview tab of this description, before it is
     submitted.

     Commented out because that directory is a repository addon and is not here
     by default. A link to a template that does not exist opens a blank
     description with nothing explaining why, which is worse than no link.
     Uncomment the line below once the directory is adopted, keep only the
     templates actually taken, and add any others to it. -->

<!-- Not an ordinary change? [Release](?expand=1&template=release.md) · [Hotfix](?expand=1&template=hotfix.md) -->

<!-- The reader is a reviewer agent that checks the diff against this body, and
     later agents that find it by commit. It already has the diff and every
     commit body, so carry only what neither shows, across the whole change,
     in the terms of the repository's GLOSSARY.md where it has one. Every
     heading stays; a section with nothing in it says "None", which is a claim
     a reviewer can check where an absent section is not. -->

## Intent

<!-- `Closes #N` for each issue this PR finishes, one per line, or where the
     request came from when there is none. Then the problem and the outcome,
     concrete enough that a gap between this and the diff is visible. A diagram
     only when the change's shape spans files and no single hunk shows it. -->

## Read first

<!-- `path:line` on the head side (`path` for a new file) where the change
     pivots, kept current after fix commits. Then the mechanical parts --
     renames, generated, vendored -- marked as safe to skim. -->

## Decisions

<!-- "X over Y, because Z", only for what could reasonably have gone another
     way. It stops Y returning as a review finding or a rewrite. "None" claims
     the change had no judgment calls, which is rarely true. -->

## Out of scope

<!-- What a reviewer would otherwise report as missing, with the follow-up
     issue. The linked issue wins where the two conflict. -->

## Verified

<!-- `command`, rerunnable from the repository root as written, and what it
     printed. Red before the change and green after is the strongest evidence;
     a screenshot for a visual change. A summary of results is not evidence. -->

## Not verified

<!-- What was not checked, and why: untested paths, cases skipped, anything
     that ran only locally or could not be exercised. This section is more use
     to a reviewer than the one above. -->

## Merge danger

**Door:** <!-- two-way: a revert restores everything | one-way: data, schema, a published interface, or anything a revert leaves changed -->

**Blast radius:** <!-- the callers, consumers, and surfaces this reaches -->
