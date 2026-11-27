# The gate

Status: accepted with the umbrella spec; built in phase P00. Binding. Changing this file needs
Jann's approval.

The gate decides what may reach `main`. Code PRs merge on their own when every check is green.
PRs that touch protected paths wait for Jann. Claude works under Jann's GitHub account, so GitHub
cannot tell the two apart: the rules in `CLAUDE.md` that Claude never approves, never weakens
the gate and never merges a waiting PR are part of the gate.

Two words are used with a fixed meaning. A **check** is a required status check on `main`: a CI
job with that name. A **part** is one `cargo xtask` subcommand; a check runs one part or several.

## 1. Required checks

All checks are required status checks on `main` (branch protection, administrators included).
Each is a CI job of the same name, and `cargo xtask` gives its verdict locally; the commands are
those of `.loom/checks.toml`.

| Check | Runs | Parts |
|---|---|---|
| `gate` | `cargo xtask gate --skip test,coverage,ui` | `fmt`, `lints`, `commit-msg`, `secrets`, `clippy`, `docs`, `deny`, `ac-trace` |
| `test` | `cargo xtask gate --only test` | `test` |
| `coverage` | `cargo xtask gate --only coverage` | `coverage` |
| `ui` | `cargo xtask gate --only ui` | `ui` |
| `pr` | `cargo xtask pr --title <title> --body-file <file>` | `pr-title`, `pr-body` |
| `approval` | `cargo xtask approval` | `approval` |

`cargo xtask gate` runs the parts in this order: `tools`, `fmt`, `lints`, `commit-msg`, `secrets`,
`clippy`, `docs`, `deny`, `test`, `coverage`, `ui`, `ac-trace`. `tools` runs first in every
`cargo xtask gate` job and is no part of a check: it fails when a tool the selected parts need
is missing or has another version than `gate/tools.toml` pins. `approval` runs only on GitHub,
and `pr` needs a pull request title and description, so a plain `cargo xtask gate` leaves both
out. `--only` and `--skip` take part names; a name that is not a part of the gate is refused.
Claude runs `cargo xtask gate` on Windows or Linux before claiming a phase is done; it gives the
same verdicts as CI (`commit-msg` and `secrets` look at the commits since `main`).

### 1.1 Parts

| Part | What it runs |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `lints` | `cargo xtask lints`: `[workspace.lints]` in the root `Cargo.toml` equals `gate/lints.toml`; the config files equal their `gate/` copies and exist nowhere else; no `eslint.config.*` anywhere but directly in `web/`; library and binary code comes only from its crate's `src/`; `cfg` names only what `gate/cfg-names.toml` allows, and no `cfg` is an empty `any()`; no `allow` or `warn` attribute anywhere, outer or inner; no `allow` or `expect` of a guard lint or of `clippy::pedantic`/`clippy::restriction` (§2); no reasonless `#[expect]` or `#[ignore]` anywhere, macro input and code behind another OS's `cfg` included |
| `commit-msg` | `cargo xtask commit-msg`: every commit of the PR is a Conventional Commit (`CONTRIBUTING.md` §2) |
| `secrets` | `cargo xtask secrets`: gitleaks scans the commits of the PR (the push, or the branch since its merge base with `origin/main`), not the working tree; without a range, the whole history. It runs with `--ignore-gitleaks-allow`: a `gitleaks:allow` comment hides nothing, and the only exceptions are in the protected `.gitleaksignore`. A finding names the rule, file, line and commit, never the secret. |
| `clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| `docs` | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` |
| `deny` | `cargo deny --locked check` (licences, advisories, bans, sources) |
| `test` | `cargo nextest run --workspace --locked` (profile `ci` in CI, `default` locally) and `cargo test --doc --workspace --locked`. Every test runs, there is no slow-test split. Tests that need PostgreSQL use `LOOM_TEST_DATABASE_URL` (a throwaway database, never the real one). |
| `coverage` | `cargo xtask coverage`: per-crate line coverage at or above the floor in `gate/thresholds.toml`, and no crate drops more than the allowed amount below `main` (§3) |
| `ui` | `cargo xtask ui` in `web/`: `pnpm install --frozen-lockfile --ignore-scripts`, then `pnpm exec tsc -b` (which type-checks every `.ts`/`.tsx` file under `web/` outside dot-folders, tests included, through `web/tsconfig.app.json` and `web/tsconfig.node.json`), `pnpm exec eslint . --max-warnings 0` and `pnpm exec vitest run` (which runs only `*.test` and `*.spec` `.ts`/`.tsx` files outside dot-folders, `node_modules` and `dist`, so every test that runs is type-checked; `.mts`/`.cts` tests and tests in dot-folders are not run). It never runs a `package.json` script (the install passes `--ignore-scripts`, so no lifecycle script runs), because a script is unprotected text. `web/eslint.config.js` sets `noInlineConfig` and `reportUnusedDisableDirectives: "error"`, so no `eslint-disable` comment switches a rule off. |
| `ac-trace` | `cargo xtask ac-trace`: every acceptance criterion of the PR's phase has at least one test tagged `AC: <id>`; no tag names an unknown criterion (§4) |
| `pr-title` | `cargo xtask pr-title`: the title is a Conventional Commit ending with `(PNN)`, `(gate)` or `(docs)` (`CONTRIBUTING.md` §3) |
| `pr-body` | `cargo xtask pr-body`: the PR description has every section of the template, filled |
| `approval` | fails while the PR touches a protected path and lacks the label `approved` (§5) |

`pr-title` and `pr-body` run together as `cargo xtask pr`, through the same runner as the gate: one
table, one summary, and both always run.

`ac-trace` adds a result column to its table when nextest wrote a report: the report of the
profile `ci` (`default` locally). A missing report adds nothing, so a test that did not run shows
`–`; no pass is ever claimed that did not happen. `cargo xtask gate` runs `ac-trace` after `test`
for that reason. In CI the check `gate` runs no tests, so there the table has no result column;
the verdict never depends on it, because `test` fails for a failing test.

### 1.2 Docs-only rule

A pull request that changes only documents cannot change the verdict of most parts, so those
parts report *not needed* (shown as `skip` in the table) and finish in seconds. `gate/code-paths.toml`
has three lists of globs against repo-relative paths with `/`; `*` stays in a folder, `**` crosses
folders. Non-code and `ui` match exactly; always-code ignores letter case and wins over non-code.
Everything not on the non-code list is code, so a new path is code until a protected PR adds it
to the non-code list.

#### Non-code

- `docs/**`
- `README.md`
- `CLAUDE.md`
- `.claude/**`

#### Always-code

- `docs/specs/00-umbrella.md`
- `docs/specs/phases/**`
- `docs/gate.md`
- `**/*.ts`
- `**/*.tsx`
- `**/eslint.config.*`
- `**/Cargo.toml`
- `**/Cargo.lock`
- `**/rust-toolchain`
- `**/rust-toolchain.toml`
- `**/clippy.toml`
- `**/.clippy.toml`
- `**/rustfmt.toml`
- `**/.rustfmt.toml`
- `**/.cargo/**`
- `**/.gitignore`
- `**/.gitattributes`
- `web/package.json`
- `web/pnpm-lock.yaml`

#### ui

- `web/**`
- `gate/tools.toml`
- `gate/code-paths.toml`
- `xtask/**`
- `.github/**`

#### How it decides

The same rule decides in CI and locally. Its inputs:

- **pull request in CI:** the event's head branch, and the paths changed between the merge base
  of the event's base and head and the head;
- **local run:** the current branch, and every path changed since the merge base with
  `origin/main`: committed, staged, unstaged and untracked (not ignored), so uncommitted code
  counts;
- **push, manual run, detached HEAD:** no rule; every part runs.

Before any part runs, the runner decides, in every job and locally alike:

1. The head branch starts with `phase/`: every selected part runs (`ac-trace` reads the phase from
   the branch). Only `ui` may still be not needed, by step 5.
2. The changed paths cannot be listed (a missing commit, git failing): every selected part runs,
   with a note.
3. No changed path at all: every selected part runs.
4. Every changed path is non-code and none is always-code: of the selected parts only `commit-msg`
   and `secrets` run; every other one is reported as not needed, naming `gate/code-paths.toml`.
   `commit-msg` and `secrets` always run: they read the commits, and a document can hold a secret.
5. Otherwise every selected part runs, except `ui`, which is not needed when no changed path is on
   the `ui` list.

The changed paths are those of the whole pull request or branch against the merge base, never
those of the last push, with renames as a deletion and an addition. `tools` checks only the pins
of the parts that will run. Every uncertain case adds parts; none removes one.

Why a non-code change cannot change the verdict of a part that is skipped (from reading each
part; every path it reads is code):

| Part | What it reads outside `crates/` and `xtask/` | Why non-code paths cannot change its verdict |
|---|---|---|
| `fmt` | nothing | formats only workspace `.rs` files |
| `clippy` | the umbrella, `gate/code-paths.toml` and `.github/pull_request_template.md` (`include_str!` in tests) | all code |
| `test`, `coverage` | the same three `include_str!` files, and at run time the umbrella, `docs/gate.md`, `CONTRIBUTING.md`, `docs/specs/phases/`, `.github/**`, `.loom/**`, `gate/**`, `web/.nvmrc` and `web/package.json` (the web copies of the pins), and the workspace manifests through `cargo metadata` | all code (`CONTRIBUTING.md` is on no non-code list); no test or doc test reads a non-code file |
| `docs` | nothing (rustdoc does not compile `cfg(test)`) | none |
| `deny` | Cargo files, `deny.toml`, the advisory database | Cargo files are code; a new advisory is not caused by the PR and shows at the next code PR and the next push to `main` |
| `lints` | every file name (it looks for `Cargo.toml` and tool configs anywhere) and the files in `gate/` | those names and files are always-code or code |
| `ac-trace` | the umbrella, the phase specs, every `*.ts` and `*.tsx` file, the branch name | umbrella, phase specs and TypeScript are always-code; `phase/` branches run every part |
| `ui` | `web/**`, `gate/tools.toml` (`web/.nvmrc` and `web/package.json` must equal the pins) | all on the `ui` list; the part is skipped only when none changed |
| `tools` | `gate/tools.toml` | code |

`commit-msg`, `secrets`, `pr-title`, `pr-body` and `approval` always run: they read the commits
and the pull request, not files.

### 1.3 Speed

On a warm cache a code pull request's `gate`, `test` and `coverage` each finish within 3 minutes
of the run's start; `pr` and `approval` within 20 seconds; the required jobs of a docs-only pull
request within 1 minute. Pull requests restore caches but never save them; only `main` saves.
The budgets are measured on a phase's PR and recorded in its trial record.

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
`gate/lints.toml`. Later changes need approval.

## 3. Thresholds (`gate/thresholds.toml`)

| Item | Start value |
|---|---|
| Line coverage, library crates (`loom-core`, `loom-plan`, `loom-store`, `loom-run`, `loom-gate`, `loom-context`, `loom-api`, `xtask`) | 80 % |
| Line coverage, binary crates (`loom`) | 60 % |
| Largest allowed drop of a crate below `main` | 1.0 percentage point |

Every workspace member needs a class there, so a new crate cannot escape the floors. Raising a
floor is welcome and still a protected change. Lowering one is never done to make a PR pass.

## 4. Acceptance-criterion tags

A test proves a criterion by a comment directly above it:

```rust
// AC: TU-01.1
#[test]
fn a_plan_round_trips_byte_for_byte() { … }
```

Several: `// AC: TU-01.1, TU-01.3`. TypeScript tests use the same comment. Criteria are defined
only in `docs/specs/00-umbrella.md`.

The criteria a `phase/PNN-<slug>` branch must prove are those listed in section 2 of the phase
spec `docs/specs/phases/PNN-<slug>.md`: the list items whose first word is a criterion id. Every
id must belong to that phase in the umbrella, and the list must not be empty; a wrong scope fails
loudly. Without a phase spec the umbrella's entry for the phase decides. A phase built in another
repository (its heading in the umbrella ends with (repo `owner/name`)) demands no tag here.

## 5. Protected paths — Jann approves

- `CLAUDE.md`, `.claude/**`, `CONTRIBUTING.md`
- `docs/**`, except `docs/plans/**` and `docs/trial/**`
- `.github/**`, `gate/**`, `xtask/**`, `.loom/**`, `deny.toml`, `rust-toolchain.toml`, `.cargo/**`
- `web/eslint.config.*`, `web/**/eslint.config.*`, `web/tsconfig*.json`, `web/vite.config.*`,
  `web/vitest.config.*`, `web/vitest.workspace.*`
- `.gitleaksignore`, `.gitleaks.toml`
- the `[workspace.lints]` table of the root `Cargo.toml` (checked by `approval`)

The list is `.loom/protected.toml`. `approval` reads it, and `gate/repo-settings.toml`, from the
base commit of the pull request, so a pull request cannot loosen the rules it is judged by.

Flow: CI adds the label `needs-jann` to such a PR and `approval` fails. Jann reviews and adds the
label `approved`; the check re-runs and auto-merge completes. Only Jann adds `approved`. An
approval is tied to the content it was given for: when the PR changes afterwards, CI removes
`approved`. `needs-jann` stays on: CI adds it back, and removing it only starts another run.

Crate-level READMEs and rustdoc are not protected; they are reviewed in the phase reviews.

## 6. Merging

- Squash merge only, linear history, branch deleted after merge.
- Branches must be up to date with `main` before merging.
- No direct pushes, force pushes or deletions on `main`, for administrators too.
- Claude enables auto-merge on its PRs that do not touch protected paths; the PR merges when all
  checks pass.
- The checks `loom-review`, `loom-contract` and `loom-approval` of umbrella R3, posted by the
  Loom GitHub App, come with P07; until then `approval` is the check for protected paths.
- The ruleset is `gate/ruleset-main.json` and the repository settings are
  `gate/repo-settings.toml`. `cargo xtask github check` compares the live repository with them;
  `cargo xtask github apply` makes the repository like them, and is only run after Jann has said
  so. Nobody changes the live settings by hand.

## 7. Process requirements outside CI

- Every phase follows `.claude/skills/loom-phase/SKILL.md`.
- The final whole-branch review loops until a fresh reviewer finds nothing (CLAUDE.md).
- The PR description carries the final review summary, every ruling Claude made and a link to
  the trial record of the step.
