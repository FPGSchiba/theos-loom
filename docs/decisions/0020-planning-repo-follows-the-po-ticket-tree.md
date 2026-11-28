# 0020 · The planning repo follows the BMad PO's ticket tree

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P01 refinement

## Decision

Jann plans with BMad and its PO agent (Sofia), and wants her tools to work on the planning repo.
So the planning repo uses her ticket tree as it is: per component one initiative folder with its
`tickets/` folder, `tickets.toml` files in build order, `epic-<slug>/epic-<slug>.md`, story,
spike and bug files with her templates (Description, Requirements, Boundaries, References,
Notes), and her plan files beside them. Loom reads that tree and adds only a few front-matter keys
of its own, which her tools ignore (decisions 0021 and 0024).

Umbrella §2.1 says so from version 9. TU-01.1 keeps its wording: "every file of §2.1" now means
the files of this layout.

## Options considered

- **The umbrella's own layout** (`epics/<slug>/epic.md`, `stories/<slug>.md`) with her sections
  inside the files: no umbrella change, but `tickets.py` and her checks would not run on the
  planning repo, so Jann could use her prompts but not her tools.
- **Her layout everywhere** (chosen): her tools keep working, and the BMad fork needs no layout
  change of its own.

## Consequences

- The epic's goal, scope and epic-level ACs of §2 map onto her sections: the goal is the
  Description and Outcome, scope and out of scope are the Boundaries, the epic-level ACs are her
  numbered requirement lines `R<n>`. A story's ACs are her numbered criteria.
- A story that is only a `tickets.toml` entry (not pulled into a file) is a planned story with no
  criteria yet; decision 0022 says when that is an error.
- When her format changes, Loom's reader follows it in a story of its own.
