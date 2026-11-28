# 0021 · `loom: ready | paused` starts and pauses work; priority is the build order

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P01 refinement

## Decision

An epic or story becomes work for Loom when it carries `loom: ready` in its front matter (a
story may carry it on its `tickets.toml` entry instead), and `loom: paused` pauses it. No `loom`
key means draft. Dropped is the BMad PO's own `status: dropped`: in an epic's file, or in a
story's plan file. Priority is the build order, the order of the tables in the `tickets.toml`;
there is no separate `priority` key.

## Options considered

- **Reuse the PO's `refined: true` as ready**: no new key, but every epic she marks refined
  would start as soon as it is merged, and there would be no way to pause.
- **The umbrella's `status: ready`**: the PO's tools refuse any epic status other than
  `in-progress`, `done` and `dropped`.
- **A Loom key** (chosen): starting work is Jann's explicit act, separate from "the detail is
  agreed".

## Consequences

- The board's moves edit one field each: the `loom` key, the PO's `status: dropped`, or the
  card's place in its `tickets.toml` (decision 0025).
- Build order and priority can't disagree, because they are one thing.
