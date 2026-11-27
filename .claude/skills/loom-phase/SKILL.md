---
name: loom-phase
description: Use to run a phase of the theos-loom umbrella spec by hand through Loom's own loop (refine, then a fresh implementation, then a fresh review loop) and record the trial numbers. Triggers on "next phase", "run P03", "refine P01", "implement P01", "review P01", "where are we".
---

# loom-phase: one phase through the loop, by hand

Loom will later run this loop on its own (umbrella §3.1). Until then it runs by hand, and every
run is also a measurement (umbrella O0). Each step below is **its own session**. Never do two steps
in one session: a fresh implementer that has never seen the refinement is the point of the trial.

Find out which step is due from `docs/trial/PNN-<slug>.md` (it's created in step 1) and the
branch state. If Jann names a phase, use it; otherwise take the next one of stage 1 in the order of
`CLAUDE.md`.

## Step 1: Refine (session A)

1. Create the worktree and branch `phase/PNN-<slug>`.
2. Copy `docs/trial/TEMPLATE.md` to `docs/trial/PNN-<slug>.md` and fill in the start time.
3. Run superpowers:brainstorming from the phase's entry in the umbrella. Scope and ACs are fixed.
   **Jann is the decider**: every question goes to him, one at a time, in plain words, with your
   recommendation and reason. Note each question and his answer in the trial record.
4. Write the phase spec to `docs/specs/phases/PNN-<slug>.md` and run the builder check with a
   fresh subagent. Sort its questions as brainstorming says; bring Jann only his own.
5. Jann approves the phase spec (decider approval). Then run superpowers:writing-plans into
   `docs/plans/PNN-<slug>.md`, with T1–T13 and the phase's ACs copied verbatim, and with the files
   and areas the phase will touch listed at the top.
6. Jann approves the plan. Record anything he decided as a decision in `docs/decisions/`.
7. Commit spec, plan, decisions and the trial record on the branch. Record the step's usage (below)
   and **end the session**. Tell Jann: "Refinement done. Start a fresh session and say `implement
   PNN`."

## Step 2: Implement (session B, fresh)

1. Read only: `CLAUDE.md`, the umbrella's §8 and the phase's entry, the phase spec, the plan, the
   decisions it names, and the code. Do not read the refinement's conversation; it isn't available
   anyway.
2. Run superpowers:subagent-driven-development over the plan, with its per-task reviews.
3. If the plan has a gap, **stop and ask Jann** (the decider) instead of patching around it. A plan
   change he allows is committed to the plan file, so the PR shows it. Note it in the trial record.
4. Run the full local gate, read its output, push and open the PR (filled template, every
   `Ruling:` listed). Don't enable auto-merge yet.
5. Record the step's usage and **end the session**. Tell Jann: "Implementation done. Start a fresh
   session and say `review PNN`."

## Step 3: Review (session C, fresh)

1. Run the final review loop of `CLAUDE.md`: fresh whole-branch reviewer, one fix subagent with the
   complete list, a new fresh reviewer, until a reviewer reports nothing. Cap: 5 rounds.
2. Record each round's findings (counts by severity, one line each) in the trial record.
3. When clean and CI is green: enable auto-merge, unless the PR touches a protected path; then it
   waits for Jann.
4. Ask Jann for his verdict and note it in the trial record: **would he have merged it as is?**
   (yes / with changes / no), plus a line on why.
5. Record the step's usage and the phase's totals.

## Recording usage

After each step, record its tokens and API-equivalent cost and the wall time. In an interactive
session, take them from `/cost` or the session's usage display. In a headless run
(`claude -p --output-format json`), take `total_cost_usd` and the token counts from the result.
If a number isn't available, write `n/a` and why. Never guess.
