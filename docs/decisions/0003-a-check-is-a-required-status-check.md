# 0003 · A check is a required status check

Date: 27 Nov 2026 · Decided by: Jann · Surfaced by: P00 refinement

## Decision

In Loom's gate, a **check** is a required status check on `main`: one CI job, declared in
`.loom/checks.toml`. P00 has six: `gate`, `test`, `coverage`, `ui`, `pr` and `approval`.
`docs/gate.md` lists these six as the checks and says, for each, what it runs (`fmt`, `clippy`,
`lints`, `ac-trace`, `commit-msg` and the rest, each a `cargo xtask` subcommand). TU-00.2 holds
as written: every check of `docs/gate.md` is a required status check.

## Options considered

- Reword TU-00.2 the way theos-editor reworded its CE-08.3 ("every check runs in a required
  status check"). That changes an acceptance criterion in the umbrella and needs a protected PR
  first.
- One CI job per check, as the editor had before its P50. That is against D4 of the P00 spec
  (lean and fast CI).

## Consequences

The word means the same in `docs/gate.md`, `.loom/checks.toml` and the umbrella's R2 and R3. The
xtask subcommands that a check runs are its parts; `docs/gate.md` names them as such. A test keeps
`docs/gate.md`, `.loom/checks.toml`, `gate/ruleset-main.json` and the workflow jobs equal.
