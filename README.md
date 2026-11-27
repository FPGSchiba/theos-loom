# theos-loom

Self-hosted harness that runs Claude Code agents on planned work around the clock: epics from a
planning repo are refined, built and reviewed by fresh agents, then merged behind enforced quality
gates. Rust (Axum) backend, React cockpit.

**Status:** stage 1, the trial. Loom is being built by hand with the loop it will later run on
its own, and every phase is measured (`docs/trial/`).

## Where things are

| Path | What |
|---|---|
| `docs/specs/00-umbrella.md` | The binding spec: phases, acceptance criteria, constraints |
| `docs/specs/phases/` | Approved phase specs |
| `docs/plans/` | Phase plans |
| `docs/decisions/` | Jann's decisions |
| `docs/trial/` | Trial record per phase: usage, review rounds, verdict |
| `CLAUDE.md` | Rules for Claude sessions in this repo |
| `.claude/skills/loom-phase/` | Runs one phase through refine → fresh implement → fresh review |

## Running a phase

Open Claude Code in this repo and say `next phase` (or `refine P00`). The skill tells you when to
start a fresh session for the next step.

## License

MIT. See [LICENSE](LICENSE).
