# Trial record: P01 · Planning repo model

Started: 28 Nov 2026, 14:53 · Finished: –

## Step 1: Refine

| | |
|---|---|
| Tokens (in / out) | n/a: `/cost` can't be read from inside the session; Jann fills it from `/cost` |
| Cost (API-equivalent) | n/a: same reason |
| Wall time | 14:53 to 15:40 (about 47 min), with PRs #11 (umbrella v9, decisions 0020–0025) and #13 (phase spec) |
| Builder-check questions | 4 (plus 3 small ones the builder would settle itself) · 0 went to Jann (settled as rulings R-n to R-r) |

Questions to the decider (Jann):

| # | Question | Answer | Decision record |
|---|---|---|---|
| 1 | Where do ACs live: the body with my own ids, or front matter? | Check BMad first: how does the PO agent (Sofia) write them? Then: follow her shape. | 0020, 0024 |
| 2 | Her folder layout too (her tools work; umbrella §2.1 changes), or the umbrella's layout with her sections inside? | Her layout everywhere. | 0020 |
| 3 | How does an epic become work and get paused: a Loom key `loom: ready / paused`, or her `refined: true`? Priority as her build order? | The Loom key; build order as priority. | 0021 |
| 4 | When is unfinished work an error: everywhere, or only in a ready epic (mistakes are errors everywhere)? | Only in a ready epic. | 0022 |
| 5 | How is a contradicting component directive marked, and is it an error or an allowed override? | Named directives, a `Contradicts: <name>` line, reported as an error. | 0023 |
| 6 | Cross-component prerequisites: a Loom key `needs:` by project-unique `key`, criteria ids key plus number? | Agreed. | 0024 |
| 7 | Fixtures: copies of the real settings and directive files, or a made-up project? | Claude decides. Ruling: a made-up project (Loom is project-neutral; no drift from the real files). | – |
| 8 | May I open the protected umbrella and decisions change? | Yes; coordinate with P03's open umbrella PR (#10). Done as #11, stacked on #10, decisions from 0020 on. | – |
| 9 | TU-20.3 says a board move commits "that front-matter field"; reword it to "the one field it changes" (`loom`, `status: dropped`, place in `tickets.toml`)? | A: reword. | 0025 |
| 10 | Approve the phase spec (with rulings R-a to R-r, four of them from the builder check)? | Yes. | – |
| 11 | Approve the plan (14 tasks; the phase spec goes in its own PR, #13, because `docs/specs/**` is protected)? | Yes; open the spec PR. | – |

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
