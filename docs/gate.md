# The gate

Status: accepted with the umbrella spec; built in phase P00. Binding. Changing this file needs
Jann's approval.

The gate decides what may reach `main`. Code PRs merge on their own when every check is green.
PRs that touch protected paths wait for Jann. Claude works under Jann's GitHub account, so GitHub
cannot tell the two apart: the rules in `CLAUDE.md` that Claude never approves, never weakens
the gate and never merges a waiting PR are part of the gate.

## 1. Required checks

All checks are required status checks on `main` (branch protection, administrators included).

| Check | What it runs | OS |
|---|---|---|
| `fmt` | `cargo fmt --all --check` | Linux |
| `clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` | Linux |
| `test` | `cargo nextest run --workspace --locked` (profile `ci` in CI, `default` locally; both leave out the slow tests) and `cargo test --doc --workspace --locked` | Linux |
| `slow-tests` | `cargo xtask slow-tests`: `cargo nextest run --workspace --locked --profile slow --no-tests=warn`, only the slow tests; on a pull request only when it can change their outcome (below) | Linux |
| `docs` | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | Linux |
| `deny` | `cargo deny --locked check` (licences, advisories, bans, sources) | Linux |
| `engine-free` | `cargo xtask engine-free`: no engine crate in the tree without feature `engine` | Linux |
| `coverage` | `cargo xtask coverage`: per-crate line coverage ≥ floor in `gate/thresholds.toml`, and no crate drops more than the allowed amount below `main` | Linux |
| `lints` | `cargo xtask lints`: `[workspace.lints]` in the root `Cargo.toml` equals `gate/lints.toml`; the config files equal their `gate/` copies and exist nowhere else; library and binary code comes only from its crate's `src/`; `cfg` names only what `gate/cfg-names.toml` allows, and no `cfg` is an empty `any()`; no `allow` or `warn` attribute anywhere, outer or inner; no `allow` or `expect` of a guard lint or of `clippy::pedantic`/`clippy::restriction` (§2); no reasonless `#[expect]` or `#[ignore]` anywhere, macro input and code behind another OS's `cfg` included (P00 spec §5.10). | Linux |
| `ac-trace` | `cargo xtask ac-trace`: every acceptance criterion of the PR's phase has at least one test tagged `AC: <id>`; no tag names an unknown criterion | Linux |
| `pr-body` | `cargo xtask pr-body`: the PR description has every section of the template, filled | Linux |
| `pr-title` | `cargo xtask pr-title`: the title is a Conventional Commit ending with `(PNN)`, `(gate)`, `(docs)` or a release title (`CONTRIBUTING.md` §3) | Linux |
| `commit-msg` | `cargo xtask commit-msg`: every commit of the PR is a Conventional Commit (`CONTRIBUTING.md` §2) | Linux |
| `approval` | fails while the PR touches a protected path and lacks the label `approved` | Linux |
| `ui` | from P11: `pnpm install --frozen-lockfile`, `svelte-check`, `eslint`, `vitest`, Playwright component tests | Linux |

**Slow tests** are tests that take minutes, such as the map generator's parity and golden runs
(P08 spec §5.14). A test is slow when a segment of its name, the path nextest shows, starts with
`slow_`: `maps::parity::slow_recipes_match`, or every test in a module `slow_runs`. The nextest
profiles in `gate/nextest.toml` split the suite: `default` and `ci` leave the slow tests out, and
the profile `slow` runs only them, so every test runs in exactly one of `test` and `slow-tests`.
`coverage` uses the profile `default` and so measures without the slow tests. A test that is not
slow has no name segment that starts with `slow_`, or `test` would leave it out.

`ac-trace` adds a result column to its table when nextest wrote reports: it merges the report of
the profile `ci` (`default` locally) with that of `slow`, so a criterion proven only by a slow
test shows `pass` or `FAIL` from the slow run. A report that is missing adds nothing, so a slow
test that did not run shows `–`; no pass is ever claimed that did not happen. `cargo xtask gate`
runs `ac-trace` after `test` and `slow-tests` for that reason. The CI job `ac-trace` runs no
tests and receives no reports from the other jobs, so there the table has no result column; the
verdict never depends on it, because `test` and `slow-tests` fail for a failing test.

On a pull request, `slow-tests` runs the slow tests only when the pull request changes, since its
merge base, a file of a workspace crate with slow tests (one of its `.rs` files contains `slow_`),
of a workspace crate such a crate depends on, or one of `Cargo.toml`, `Cargo.lock`,
`rust-toolchain.toml`, `.cargo/`, `.config/`, `.github/`, `gate/` and `xtask/`. Otherwise it
passes with a note that says so, without building anything. The job runs and reports on every
pull request either way, so the required check is never missing. A push to `main`, a manual run,
the `platforms` schedule and `cargo xtask gate` always run the slow tests.

**Windows and macOS** are not in the per-PR checks, because their runner minutes cost two and ten
times as much as Linux on a private repository. Instead the workflow `platforms` runs `clippy`,
`test`, `slow-tests`, `lints`, `ac-trace` and (from P11) `ui` on `windows-latest` and
`macos-latest`, and `test` and `slow-tests` also on `macos-15-intel`: for `main` once a week, on
demand, and on a PR when the label `windows` or `macos` is added (each label runs its own OS). The
full test suite is `test` and `slow-tests` together; `release.yml` runs both on all three before
every release (`docs/release.md`).
These runs are not required checks. A red run is fixed before the next release; it is listed in
`docs/status.md` under "Needs Jann" until then. `lints` and `ac-trace` run there too because
they read what the compiler builds on the machine they run on, and code behind `#[cfg(windows)]`
or `#[cfg(target_os = "macos")]` is only built there.

`cargo xtask gate` runs every check except `pr-body`, `pr-title` and `approval` locally, on
Windows and Linux, with the same results as CI (`commit-msg` checks the commits since `main`).
`slow-tests` always runs the slow tests there, so the local gate is never more lenient than CI.
Claude runs it before claiming a phase is done.

## 2. Lints (`gate/lints.toml`, mirrored into `Cargo.toml`)

At least: `unsafe_code = "forbid"`, `missing_docs = "deny"`, `clippy::all` and
`clippy::pedantic` as warnings (errors under `-D warnings`), `clippy::unwrap_used`,
`clippy::expect_used`, `clippy::panic`, `clippy::todo`, `clippy::unimplemented`,
`clippy::dbg_macro`, `clippy::print_stdout`, `clippy::print_stderr` denied outside tests and
binaries' `main` (use `#[expect(…, reason = "…")]` where one is justified). Four lints guard the
others: `unexpected_cfgs` is forbidden, and `clippy::allow_attributes`,
`clippy::allow_attributes_without_reason` and `clippy::ignore_without_reason` are denied (derives
such as clap's allow the lint groups that contain them, so they cannot be forbidden). `lints`
refuses every `allow` and `warn` attribute in our code, and every `allow` or `expect` that names
one of the four or the groups `clippy::pedantic` and `clippy::restriction` that contain them. It
also asks for a reason on every `#[expect]` and `#[ignore]`, also where clippy does not look, so
none of the four can be switched off. The result: no `#[allow]` or `#[warn]` anywhere, a reason
on every `#[expect]` and `#[ignore]`, and no unknown cfg name. Lint levels live only in
`gate/lints.toml`. P00 writes the final list; later changes need approval.

## 3. Thresholds (`gate/thresholds.toml`)

| Item | Start value |
|---|---|
| Line coverage, library crates | 80 % |
| Line coverage, binary crates (`theos`, `theos-mcp`, `theos-editor`) | 60 % |
| Largest allowed drop of a crate below `main` | 1.0 percentage point |

Raising a floor is welcome and still a protected change. Lowering one is never done to make a PR
pass.

## 4. Acceptance-criterion tags

A test proves a criterion by a comment directly above it:

```rust
// AC: CE-09.1
#[test]
fn imported_records_round_trip_byte_for_byte() { … }
```

Several: `// AC: CE-09.1, CE-09.3`. TypeScript tests use the same comment. Criteria are defined
only in `docs/specs/00-umbrella.md`.

## 5. Protected paths — Jann approves

- `CLAUDE.md`, `.claude/**`, `CONTRIBUTING.md`
- `docs/**`, except `docs/plans/**` and `docs/status.md`
- `.github/**`, `gate/**`, `xtask/**`, `deny.toml`, `rust-toolchain.toml`, `.cargo/**`
- `CHANGELOG.md`, `release-plz.toml`, `cliff.toml` (so every release PR waits for Jann)
- `design/**`
- the `[workspace.lints]` table of the root `Cargo.toml` (checked by `lints`)

Flow: CI adds the label `needs-jann` to such a PR and `approval` fails. Jann reviews and adds the
label `approved`; the check re-runs and auto-merge completes. Only Jann adds `approved`.
`needs-jann` stays on: CI adds it back, and removing it only starts another run.

Crate-level READMEs and rustdoc are not protected; they are reviewed in the phase reviews.

## 6. Merging

- Squash merge only, linear history, branch deleted after merge.
- Branches must be up to date with `main` before merging.
- No direct pushes, force pushes or deletions on `main`, for administrators too.
- Claude enables auto-merge on its PRs that do not touch protected paths; the PR merges when all
  checks pass.
- Release PRs are merged by Jann only. Tags are created only by the release pipeline
  (`docs/release.md`); nobody creates, moves or deletes a tag by hand.

## 7. Process requirements outside CI

- Every phase follows `.claude/skills/theos-phase/SKILL.md`.
- The final whole-branch review loops until a fresh reviewer finds nothing (CLAUDE.md).
- The PR description carries the final review summary and every ruling Claude made.
