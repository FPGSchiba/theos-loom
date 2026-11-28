# 0012 · Each run gets its own local clone

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P03 refinement

## Decision

A run's "fresh git worktree" (TU-03.1) is a local clone of a shared mirror of the component repo,
made with `git clone --local` (objects hard-linked, so it is fast and small). It holds the story
branch and pushes to GitHub. Each container sees only its own clone.

## Options considered

- Real `git worktree`s: their `.git` points back to the shared repository, which every container
  would then need mounted, so each run could see and change the others' branches; and git
  refuses to check out one branch in two worktrees, which two concurrent runs of one story need.

## Consequences

The same reading holds for T4's "its own git worktree". Parallel subagents inside one run that
need worktrees of their own make them inside the run's clone.
