# theos-loom — instructions for Claude

This repo builds Loom, the harness that runs Claude Code agents on Jann's projects. Claude builds
it; Jann owns the architecture. Read this file fully at the start of every session.

Loom is being built **by hand with the loop it will later run on its own**. Building it is the
trial that decides whether the loop is good enough (umbrella §9, item O0). Every phase is run
with the project skill `.claude/skills/loom-phase/SKILL.md`.

## Sources of truth

1. `docs/specs/00-umbrella.md`: phases, acceptance criteria (ACs), global constraints T1–T13.
2. `docs/decisions/`: Jann's decisions, numbered `NNNN-<slug>.md`.
3. `docs/specs/phases/PNN-<slug>.md`: the approved spec of a phase.
4. `docs/plans/PNN-<slug>.md`: the plan of a phase.
5. `docs/trial/`: the trial record, one file per phase.

Higher beats lower. Once P00 has written them, `docs/gate.md` and `CONTRIBUTING.md` rank with the
umbrella.

## How work is done

- Every piece of work is a phase of the umbrella, run with `loom-phase`. It has three steps, each
  in its **own session**: refine, implement, review. A session never does two of them.
- Stage 1 (umbrella §9) is the only work for now. Its order: P00 → P01 → P02 and P03 → P04 → P05
  → P06 → P07 → TU-08.1 and TU-08.2 → P13 → P14.
- Never work on `main`. Never push to `main`. Every phase runs in its own git worktree from its
  first step on (`.worktrees/<branch>` if the session has no native worktree tool).
- **One writer per worktree.** Every agent that writes files gets its own worktree. Subagents may
  share their session's worktree only when they run one after another. Agents that run in
  parallel and write each get a worktree of their own, on a branch of their own, and their work
  is merged back into the phase branch one at a time. Reviewers that only read may share.
- During the trial, **Jann answers in the decider's place**: questions during refinement, and
  approval of the phase spec and plan. Each answer that decides something is recorded as a
  decision in `docs/decisions/` and listed in the phase's trial record.

## Hard rules: the gate

Claude works under Jann's GitHub account, so GitHub can't tell the two apart. Breaking one of
these rules is never acceptable, also not to unblock work.

1. Never weaken a check: never lower a threshold, disable or skip a check, delete or weaken a
   test to make it pass, add `#[allow]`, or add `#[ignore]` / `#[expect]` without a real reason
   in the attribute.
2. Never merge, enable auto-merge on, or ask anyone to merge a PR that touches a protected path.
   Until P00 defines them, protected paths are: `CLAUDE.md`, `.claude/**`, `docs/specs/00-umbrella.md`,
   `docs/decisions/**`, `.github/**` and everything under `gate/` and `xtask/`.
3. Never change protected paths inside an implementation PR. They get their own PR.
4. Never change an AC. If an AC is wrong or impossible, stop and say so.
5. Never use `--no-verify`, force-push to a shared branch, or rewrite history on `main`.
6. Never claim a check passed without having run it and read its output
   (superpowers:verification-before-completion).
7. Never write lore, design concepts, world content or art for any project (umbrella §1.1).

## Changes to the superpowers loop (they override the skills)

- **Locations.** Phase specs: `docs/specs/phases/PNN-<slug>.md`. Plans: `docs/plans/PNN-<slug>.md`.
  Not `docs/superpowers/`.
- **Brainstorming** for a phase starts from the phase's entry in the umbrella. Its scope and ACs
  are fixed; the phase spec decides how.
- **Plans** copy the umbrella's global constraints (T1–T13) and the phase's ACs verbatim. Every
  task names the ACs its tests prove; tests carry `// AC: <id>`.
- **Execution** is superpowers:subagent-driven-development, in a fresh session that has never
  seen the refinement.
- **The final review is a loop**: a fresh whole-branch reviewer (most capable model), one fix
  subagent with the complete list of findings, then a new fresh reviewer, until one reports no
  findings. A reviewer may reopen a ruled finding only with new evidence. After 5 rounds, stop:
  the PR becomes a draft with the open findings listed.
- **Finishing** is always a PR. Enable auto-merge only when hard rule 2 allows it.
- **Model selection**: plans and final reviews on the most capable model; task reviewers on a
  mid-tier model; implementers on the cheapest model that fits the task.

## Engineering directives

They decide how something gets built, before any code is written. They bind every session and
subagent; put them in subagent briefs.

1. **Maintainability and extendability come first.** Small modules with one job, shared inputs
   built in one place; a new rule or check is one new module plus one line that registers it.
2. **Use what exists before building anything.** Configure a standard tool first, then a
   well-known crate, then custom code. Never hand-write a parser for a format that has a real one.
   Every design names, for each part, the tool or crate that does it, or why custom code is needed.
3. **When fixes keep leaking, change the approach.** If reviews keep finding variants of the same
   problem, name the class and run the full loop for a new approach instead of patching.
4. **The gate keeps quality high; it is not a fortress.** It catches the everyday shortcuts and
   never flags ordinary code. Deliberate, exotic evasion is left to the reviews.
5. **State lives in git and PostgreSQL, never in a model's context** (T6).
6. **Gates are enforced in code, not in prompts.** If a rule matters, it's a check.

## Code conventions

- Rust edition 2024, toolchain from `rust-toolchain.toml`. Library errors with `thiserror`;
  `anyhow` only in binaries. Logging with `tracing`. No `unwrap`/`expect` outside tests.
- Small files with one responsibility. Public items documented. Unit tests next to the code,
  behaviour tests in `tests/`, fixtures in `crates/<crate>/tests/fixtures/`.
- Tests that need PostgreSQL use a throwaway database, never the real one (T4b).
- The UI lives in `web/`: React, TypeScript, pnpm.
- Conventional Commits for every commit and PR title, with a `Phase: PNN` footer and the Claude
  trailers. Until P00 writes `CONTRIBUTING.md`, branches are `spec/PNN-<slug>`,
  `phase/PNN-<slug>`, `docs/<slug>` and `fix/<slug>`.

## When to stop and ask Jann

Only for: an irreversible or destructive action; a security-sensitive action; a protected-path
change; an AC that is wrong or impossible; a final review loop that hit its cap; and, during the
trial, every question and approval the decider would otherwise handle. Everything else is decided
and recorded as `Ruling: <what> — <why> — <cost if wrong>` in the plan, and listed in the PR.

## Language

English for code, docs, commits and PRs. README files are named README.
