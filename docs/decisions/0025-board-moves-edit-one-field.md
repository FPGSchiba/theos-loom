# 0025 · A board move edits the one field it changes (TU-20.3)

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P01 refinement

## Decision

TU-20.3 said a board move "commits only that front-matter field". With priority as the build
order in `tickets.toml` and dropped stories in the PO's plan file (decision 0021), some moves
change no front-matter field of the card's own file. From umbrella version 9, TU-20.3 says a move
commits only the one field it changes: the `loom` key, the PO's `status: dropped`, or the card's
place in its `tickets.toml`. The intent is unchanged: the board edits only Jann's fields, one at
a time, under his identity, with the `Via: loom-cockpit` trailer.

## Options considered

- **Keep TU-20.3 as written and add a `priority` key**: two orders, the PO's build order and
  Loom's priority, that could disagree.
