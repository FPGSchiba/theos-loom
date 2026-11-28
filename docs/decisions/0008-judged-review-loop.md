# 0008 · The final review loop judges its findings and ends as best effort

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P00 verdict

## Decision

- Every finding of a final reviewer is judged: fix it, or rule it not worth fixing, with a
  `Ruling:` that says why. Minor findings that change no behaviour and risk nothing are ruled,
  not fixed. The loop stops when a reviewer reports nothing worth fixing.
- After 5 rounds: one last fix wave and a targeted review of it, then stop. The review was best
  effort; the PR stays ready (not draft), lists its open findings, and doesn't wait for Jann
  because of the cap. Five fresh reviews give high enough quality.

## Options considered

- Loop until a reviewer reports no findings at all, and turn the PR into a draft for Jann at 5
  rounds (the rule until now). In P00, fresh reviewers found new minor issues in every round,
  so the loop ran to its cap and beyond, and each round needed Jann.

## Consequences

`CLAUDE.md`, the loom-phase skill and the PR template say so. The umbrella's TU-07.1 (P07: "until
one is clean, capped at 5 rounds") still describes Loom's own review service the old way; it is
left as it is until Jann decides whether P07 follows this rule.
