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
| Tokens (in / out) | |
| Cost (API-equivalent) | |
| Wall time | |
| Plan gaps raised | |
| Plan changes allowed | |

## Step 3: Review

| Round | Critical | Important | Minor | Notes |
|---|---|---|---|---|
| 1 | | | | |

| | |
|---|---|
| Tokens (in / out) | |
| Cost (API-equivalent) | |
| Wall time | |
| CI green on first push | yes / no |

## Verdict

- **Would Jann have merged it as is?** yes / with changes / no
- Why:

## Totals

| Tokens | Cost | Wall time | Jann's time |
|---|---|---|---|
| | | | |
