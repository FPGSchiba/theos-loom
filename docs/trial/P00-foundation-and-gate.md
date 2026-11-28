# Trial record: P00 · Foundation and gate

Started: 27 Nov 2026, 11:15 · Finished: –

## Step 1: Refine

| | |
|---|---|
| Tokens (in / out) | Brainstorming and spec draft (questions 1–3): n/a, they ran inside the long planning session, so their usage can't be separated. Spec approval and plan (questions 4–7, own session, from `/cost`): Opus 5.5 150 in + 12.6M cache read + 295.2k cache write / 139.9k out; Haiku 5.5 2.2k in / 22 out |
| Cost (API-equivalent) | Planning-session part: n/a, same reason. Own session: $7.67 |
| Wall time | Planning-session part: n/a. Own session: 41 min 49 s (API 20 min 56 s) |
| Builder-check questions | 5 · 0 went to Jann (all settled as rulings R-h to R-l) |

Questions to the decider (Jann):

| # | Question | Answer | Decision record |
|---|---|---|---|
| 1 | Where does CI run: a self-hosted runner, or GitHub's hosted runners? | Hosted runners; the repos go public, so minutes are free. Keep CI lean and fast anyway. | 0001 |
| 2 | Which licence? | MIT, for theos-loom, theos-editor and Theos-Engine. | 0002 |
| 3 | Approve the short design (mirror the editor's gate, CI layout, caching)? | Yes: it mirrors the editor, whose gate works well. | – |
| 4 | TU-00.2 says every check of `docs/gate.md` is a required status check, but in the P50 layout `fmt`, `clippy` and the rest run inside the job `gate`. Is a "check" the required status check (A), do we reword TU-00.2 (B), or one job per check (C)? | A: a check is a required status check (`gate`, `test`, `coverage`, `ui`, `pr`, `approval`); the xtask subcommands are its parts. | 0003 |
| 5 | Where does the local gate run: Windows and Linux with the editor's Windows handling kept (A), only in WSL (B), or nowhere until Loom's VM exists (C)? | A: Windows and Linux; CI on Linux only. | 0004 |
| 6 | Approve the phase spec (with D5, D6 and the fixes found against the editor's code)? | Yes. | – |
| 7 | Approve the plan, including the toolchain's default linker instead of `mold` (a change to the approved spec)? | Yes. | 0005 |

## Step 2: Implement

| | |
|---|---|
| Tokens (in / out) | From `/cost`: Opus 5.5 524 in + 70.4M cache read + 470.3k cache write / 189.9k out; Sonnet 5.5 898 in + 40.0M cache read + 2.4M cache write / 416.1k out; Haiku 5.5 220.5k in + 1.0M cache read + 308.3k cache write / 34.7k out |
| Cost (API-equivalent) | $35.90 (Opus $21.65, Sonnet $14.17, Haiku $0.09) |
| Wall time | 1 h 59 min 2 s (API 1 h 23 min 34 s); commits from 12:29 to 14:11 |
| Plan gaps raised | At least 4: the four below. Gaps raised and refused would not show in git, and the session's own record of them wasn't written. |
| Plan changes allowed | 4, in `5cd072c` and `ae43a7c`: removing `BranchKind::Release` moves from Task 3 to Task 5, so xtask compiles after every task; a list a document mirrors changes in the same task; the xtask cache key also hashes the root `Cargo.toml` (P50 missed it); the commit subjects the plan gave Tasks 6 and 14 started upper-case, which `commit-msg` refuses. |

Cold CI run on P00's pull request (`7547d98`, 27 Nov, from the start of each job to its end):
`gate` 2:12, `test` 3:16, `coverage` 3:21, `ui` 1:47, `pr` 1:25, `approval` 1:32 (red, as designed:
the PR touches protected paths and has no `approved` label yet). Timeouts were set from these in
`7d66703`. The warm-cache budgets are measured on P01's PR (R-l).

## Step 3: Review

| Round | Critical | Important | Minor | Notes |
|---|---|---|---|---|
| 1 | 0 | 3 | 3 | Two need Jann: protect every config that can switch off `ui` (0006), no inline suppressions in `secrets` and `ui` (0007). |
| 2 | 0 | 0 | 5 | Jann allowed the fix to edit `.github/workflows/ci.yml` and `deny.toml` (caches saved on PRs, against §5.7). |
| 3 | 0 | 2 | 3 | `pnpm install` ran lifecycle scripts; a docs-only PR could add an `eslint.config.*` that breaks `lints`. Jann allowed the edits to `gate/` and `docs/gate.md`. |
| 4 | 0 | 0 | 5 | Missing Rulings for round 3's changes, a wrong TU-00.2 tag, docs gaps, gitleaks outside `process.rs`. |
| 5 | 0 | 0 | 1 | Cap reached without a clean reviewer: tests outside `web/src` ran but weren't type-checked. Jann allowed one more fix wave with a targeted review, and kept the PR ready instead of draft. |
| 6 (targeted) | 0 | 1 | 0 | Widening `tsc` left `.mts`/`.cts` tests and tests in dot-folders unchecked. Same class twice (directive 3): Jann chose to narrow Vitest to what `tsc` checks. |
| 7 (targeted) | 0 | 0 | 1 | Tests named with a leading dot still ran unchecked. Fixed as the reviewer proposed (`**/.*` in Vitest's exclude) and verified with a probe and the full gate; no further reviewer ran. |

Five whole-branch rounds, then two targeted rounds beyond the cap with Jann's approval. Fix
subagents ran on Sonnet 5.5, reviewers on Opus 5.5; the round-7 fix was a one-line change made in
the review session itself.

Questions to the decider (Jann) in this step:

| # | Question | Answer | Decision record |
|---|---|---|---|
| 8 | Protect nested ESLint configs and Vitest configs, and have `lints` refuse a nested `eslint.config.*`? | Yes, both. | 0006 |
| 9 | Run gitleaks with `--ignore-gitleaks-allow` and set ESLint's `noInlineConfig`? | Yes. | 0007 |
| 10 | May round 2's fixes edit `.github/workflows/ci.yml` and `deny.toml`? | Yes. | – |
| 11 | May round 3's fixes edit `gate/code-paths.toml`, `docs/gate.md` and `xtask/`? | Yes. | – |
| 12 | May round 4's fixes edit `docs/gate.md` and `xtask/` again? | Yes. | – |
| 13 | The loop hit its cap with 1 Minor open: draft the PR (the rule), or one more round? | One more fix wave and a targeted review; the PR stays ready, not draft. | – |
| 14 | Widening `tsc` leaked again: narrow Vitest (A), widen further (B), or stop and list it (C)? | A. | – |

| | |
|---|---|
| Tokens (in / out) | From `/cost`: Opus 5.5 968 in + 61.7M cache read + 1.4M cache write / 247.6k out; Sonnet 5.5 328 in + 10.8M cache read + 519.9k cache write / 68.8k out; Haiku 5.5 267.6k in / 24.6k out |
| Cost (API-equivalent) | $28.19 (Opus $25.08, Sonnet $3.07, Haiku $0.04) |
| Wall time | 7 h 38 min 35 s, most of it waiting for Jann's answers (API 57 min 0 s) |
| CI green on first push | yes: the first push of review fixes (`34ff557`) was green except `approval`, which waits for Jann |

## Verdict

- **Would Jann have merged it as is?** yes
- Why: he judged P00 done and merged PR #2 before this record was finished (it was reverted in
  #6 and merged again as #7, unchanged). The loop's output was good enough; the review step cost
  the most of his time, because every fix wave on a PR that is all protected paths asked for his
  approval, and reviewers kept finding minor issues. His changes to the loop: judge every
  finding and stop when none is worth fixing; after 5 rounds, one last fix and a targeted review,
  then stop as best effort with the PR ready, not draft (decision 0008).

## Totals

| Tokens | Cost | Wall time | Jann's time |
|---|---|---|---|
| Output 1.12M (refine 0.14M, implement 0.64M, review 0.34M); input almost all cache reads (refine 12.6M, implement 111.4M, review 72.5M). The brainstorming inside the planning session isn't counted. | $71.76 (refine $7.67, implement $35.90, review $28.19), without the planning-session part | 10 h 19 min wall (refine 0:42, implement 1:59, review 7:39, mostly idle); API 2 h 41 min | Little; most of it went to approving each review fix wave on protected paths (questions 10–13) |
