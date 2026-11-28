# 0022 · `loom plan check`: mistakes are errors everywhere, unfinished work only when ready

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P01 refinement

## Decision

`loom plan check` sorts its findings in two kinds.

- **Errors anywhere in the tree:** unknown front-matter keys, broken references (unknown
  component, epic or story), dependency cycles and duplicate ids. These are mistakes, never work
  in progress.
- **Errors only for an epic marked `loom: ready`, and its stories:** an epic without a goal, scope
  or requirement lines, and a story without numbered criteria. An epic with no stories at all
  stays valid, because Loom slices it.

Elsewhere in the tree, unfinished work is listed as notes and does not fail the check.

## Options considered

- **Every finding an error everywhere**: the PO's tree is full of normal half-done work (stories
  that are only a `tickets.toml` line, epics with `refined: false`), so the check would be red for
  most of the planning time and real problems would drown.
- **Unfinished stories in a ready epic go to the slicer**: blurs "stories Jann wrote are taken as
  they are" (umbrella §3.1).

## Consequences

- Stories Jann writes in a ready epic each need their criteria; an entry with only a `verify`
  line there is an error, not a hint for Loom to fill in.
