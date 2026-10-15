# Lessons learned

A lesson is a durable, repeatable, non-obvious constraint, convention, or tool quirk; leave out incident history and generic advice.
Each lesson has a short imperative `##` title, the rule, **Do:** the required action, **Why:** the consequence (only when not self-evident), and **Source:** a link to the authority.

## Lessons

## Force-add files under `.claude/`

The template-managed `.gitignore` ignores `.claude/`, so a new skill file is silently left out of the commit.
**Do:** stage it with `git add -f .claude/skills/<path>`; edit `.gitignore` only through the template.
**Source:** `.gitignore` ("Claude config" block), `.repo-template.json` (`.claude/skills/**` is `product`).
