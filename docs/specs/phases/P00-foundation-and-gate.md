# P00 · Foundation and gate — phase spec

Status: draft for Jann's approval (decider approval, trial step 1) · 27 Nov 2026
Source: `docs/specs/00-umbrella.md` §9 P00. Scope and acceptance criteria come from there; this
spec decides how.

## 1. Goal

After P00, Loom's repo has:
- the empty, documented workspace of T1, plus `web/`;
- the gate: `cargo xtask gate` and the CI jobs that decide what may reach `main`, with the same
  verdicts in both places;
- `CONTRIBUTING.md`, `docs/gate.md` and the PR template;
- branch protection and repo settings written down in `gate/` and checked against GitHub.

It is built by **porting the theos-editor's gate**, not by writing a new one. That gate is proven
and is now MIT-licensed by the same author, which is directive 2 (use what exists) applied to
Loom itself. The port takes the editor's CI layout *after* its P50 (Leaner CI), so Loom starts
lean.

## 2. Acceptance criteria (verbatim from the umbrella)

**TU-00 Workspace, CI and gate**
- TU-00.1 The workspace with the crates of T1 (empty, documented) and `web/` builds and tests in
  CI on Linux.
- TU-00.2 Every check of `docs/gate.md` is a required status check on `main`. Branch protection
  matches R3.
- TU-00.3 `cargo xtask gate` gives the same verdicts as CI.
- TU-00.4 `ac-trace`, `pr-title`, `commit-msg`, `lints` and `approval` behave as in
  theos-editor's gate, until P07 replaces `approval` with `loom-approval`.

**On R3:** the `loom-*` checks don't exist before P05–P07. For TU-00.2, "matches R3" means R3's
parts that P00 can build: squash only, linear history, branches up to date, no direct, force or
delete pushes, and every check of `docs/gate.md` required. P07 adds the three `loom-*` checks to
the ruleset.

## 3. Scope

**In:** workspace skeleton; `web/` skeleton; `xtask` with the ported checks; `gate/` data; the
workflows `ci`, `pr`, `approval` and `settings`; `docs/gate.md`; `CONTRIBUTING.md`; the PR
template; `deny.toml`; the toolchain pin; the PostgreSQL service in CI (unused until P02).

**Out:** any Loom behaviour (P01 onwards); the `loom-*` checks (P05–P07); releases (no release
phase in stage 1); Windows and macOS (Loom runs on Linux only); a local throwaway database for
`cargo xtask` (P02, the first phase that needs one).

## 4. Decisions

### 4.1 Decided by Jann (refinement, 27 Nov 2026)

| | Decision |
|---|---|
| D1 | **GitHub-hosted runners**, and theos-loom is **public**, so standard runners cost nothing. Decision 0001. |
| D2 | **MIT licence** for theos-loom, theos-editor and Theos-Engine. Decision 0002. |
| D3 | **Mirror the editor's gate.** Same checks and the same rules, adapted to Loom. |
| D4 | **Lean and fast CI anyway**: heavy caching, especially for Rust, and nothing run that can't change a verdict. |

### 4.2 Claude's calls (Rulings, for Jann to check)

- **R-a Port, don't rewrite.** Copy theos-editor's `xtask/` and `gate/` at commit `8b1b3d5`
  (main, 27 Nov 2026), then adapt them (§5.4). The port's first commit is the unchanged copy, so
  the PR shows every adaptation as a diff. The editor's P50 is approved but **not built yet** on
  that commit, so its parts (`gate --only`/`--skip`, the docs-only rule, the cached xtask, the
  three-job layout) are built in the port, following P50's spec §5. If wrong,
  the cost is a gate Loom doesn't fully understand; the reviews and `docs/gate.md` cover that.
- **R-b Three heavy jobs side by side** (`gate`, `test`, `coverage`), as in editor P50, instead of
  one job running tests and coverage together. P50 measured the split as the fastest layout, and
  on a public repo the extra job costs no minutes. If wrong, the cost is a slightly longer
  wall-clock run.
- **R-c Every job on `ubuntu-latest`.** `ubuntu-slim` saved money on a private repo; on a public
  one it only makes a cache-miss xtask build slower.
- **R-d Coverage floors as the editor's:** library 80 %, binary 60 %, largest drop 1.0 point.
  While the crates are empty they pass, as the editor's did.
- **R-e Toolchain as the editor's pin** (`1.99.1`), so both repos move together.
- **R-f The `ui` check runs from P00**, not from a later phase, because `web/` exists from P00.
- **R-h Repo contract from day one where it's cheap.** P00 already uses the paths of umbrella
  §6.2: protected paths live in `.loom/protected.toml` (R5) instead of `gate/protected-paths.toml`,
  and `.loom/checks.toml` (R2) lists every check with its local command. A secret scan (R10,
  `gitleaks`, pinned in `gate/tools.toml`) runs inside `gate`, because the repo is public. The
  rest of the contract is P06. If wrong, the cost is two file locations that P06 would move
  anyway, and moving the file `approval` reads is the expensive kind of move.
- **R-i `ac-trace` reads a phase's scope from its phase spec.** It demands tests for the ACs that
  §2 of the phase spec lists, or for every AC of the umbrella phase if there's no phase spec yet.
  Stage 1 builds only TU-08.1 and TU-08.2 of P08, so its spec lists just those two. No AC is
  changed; a phase only says which part it builds.
- **R-j `ui` is part of the local gate.** Node and pnpm are pinned in `gate/tools.toml`, and
  `tools` checks them like the Rust tools, so `cargo xtask gate` gives the same `ui` verdict
  locally (TU-00.3). `gate/code-paths.toml` gets a third list, `ui`: `ui` runs only when a
  changed path matches it (`web/**` and the UI tool configs). The editor never built `ui`, so
  this check is new code, not a port.
- **R-k P00's PR is a gate PR.** Nearly everything it writes is protected, so it waits for Jann,
  as the editor's P00 PR did. Hard rule 3 of `CLAUDE.md` is about ordinary implementation PRs.
- **R-l Speed budgets are measured on the first PR after P00 merges** (P01's), because `main` has
  no caches before P00 is merged. P00 itself records its cold-run times and sets timeouts from
  them. If P01's PR misses a budget, a `gate/` fix comes before P01 continues.
- **R-g Phase ids and branches:** `phase/PNN-<slug>`, `spec/PNN-<slug>`, `gate/<slug>`,
  `docs/<slug>` and `fix/<slug>`, as in the editor. AC ids are `TU-\d{2}\.\d+`.

## 5. Design

### 5.1 Workspace skeleton

```text
Cargo.toml              workspace (members crates/*, xtask; resolver 3); [workspace.package] with
                        version 0.0.0, edition 2024, rust-version, publish = false, repository,
                        license = "MIT"; [workspace.lints] mirrors gate/lints.toml
Cargo.lock              committed
rust-toolchain.toml     1.99.1; rustfmt, clippy, llvm-tools-preview; profile minimal
.cargo/config.toml      alias xtask = run --locked --package xtask --target-dir target/xtask-bin --
                        [target.x86_64-unknown-linux-gnu] linker clang + mold (CI and local)
.gitattributes          * text=auto eol=lf
clippy.toml             copy of gate/clippy.toml
.config/nextest.toml    copy of gate/nextest.toml
deny.toml               licences: MIT-compatible allowlist (§5.6)
crates/loom-core/       lib ┐
crates/loom-plan/       lib │
crates/loom-store/      lib │ each: Cargo.toml (inherits [workspace.package], lints.workspace),
crates/loom-run/        lib │ README, src/lib.rs or src/main.rs with crate docs stating its role
crates/loom-gate/       lib │ from umbrella T1; the binary exits 0 and has a smoke test
crates/loom-context/    lib │
crates/loom-api/        lib │
crates/loom/            bin ┘
xtask/                  lib + bin: the gate (ported)
gate/                   the gate's data (§5.3)
web/                    React app (§5.5)
.github/                workflows, the setup action, the PR template
```

### 5.2 Parts, and the tool or crate each one uses

| Part | Tool or crate | Custom code, and why |
|---|---|---|
| Every check | ported from theos-editor `xtask` | Only the adaptations of §5.4 |
| `--only`/`--skip`, docs-only rule | editor P50 spec §5.1–5.3 (not built in the editor yet) | A filter over the gate's check order; the two-list path rule |
| Build cache | `Swatinem/rust-cache` via `.github/actions/setup` (`shared-key`, `save-if` on `main` only) | none |
| Cached xtask for `pr` and `approval` | `actions/cache`, exact key, saved by `gate` on `main` (editor P50 §5.5) | none |
| Faster linking | `mold` with `clang`, installed by the setup action | none |
| Test runner, coverage, licences | `cargo-nextest`, `cargo-llvm-cov`, `cargo-deny`, pinned in `gate/tools.toml` | none |
| PostgreSQL in CI | a `postgres:18` service container in `test` and `coverage` | none |
| UI toolchain | pnpm, Vite, TypeScript, ESLint, Vitest; cache via `actions/setup-node` with `cache: pnpm` | none |
| Settings as code | the editor's `github check` / `apply` | Loom's check names and labels |

### 5.3 `gate/`

The editor's files, adapted: `lints.toml` (the same final lint list), `clippy.toml`,
`nextest.toml`, `thresholds.toml` (Loom's crates and classes: the seven libraries and `xtask` are
`library`, `loom` is `binary`), `tools.toml` (also Node, pnpm and gitleaks), `cfg-names.toml`,
`code-paths.toml` (non-code, always-code and ui lists), `ruleset-main.json`,
`repo-settings.toml`. Protected paths move to `.loom/protected.toml` (R-h), and `.loom/checks.toml`
lists every check with its local command. No `engine-crates.toml` and no
`ruleset-tags.json` (no releases yet).

**Protected paths** (`docs/gate.md` §5, `.loom/protected.toml`; `.loom/**` itself is protected): `CLAUDE.md`, `.claude/**`, `CONTRIBUTING.md`, `docs/**`
except `docs/plans/**` and `docs/trial/**`, `.github/**`, `gate/**`, `xtask/**`, `deny.toml`,
`rust-toolchain.toml`, `.cargo/**`, `web/eslint.config.*`, `web/tsconfig*.json`, and the
`[workspace.lints]` table of the root `Cargo.toml`.

**Docs-only rule** (`gate/code-paths.toml`, editor P50 §5.3), adapted: non-code adds
`docs/trial/**`; always-code adds `**/*.tsx`, `web/package.json` and `web/pnpm-lock.yaml`.

### 5.4 Adaptations to the ported xtask

| Check | Change from the editor |
|---|---|
| `engine-free`, `slow-tests` as a job, `platforms` | Removed. Slow tests (the `slow_` name rule) stay and run inside `test`. |
| `ac-trace` | Scope from the phase spec's §2 (R-i). AC ids `TU-\d{2}\.\d+`, under `### PNN ·` headings of Loom's umbrella. Loom's phase table has no repo column: a phase whose heading names another repo (P04: `FPGSchiba/bmad-method`) demands no tests here. TypeScript tags are read from `*.ts` **and `*.tsx`**. |
| `pr-title` | Suffixes `(PNN)`, `(gate)`, `(docs)`. No release titles. |
| `commit-msg` | Scopes from Loom's `CONTRIBUTING.md` (§5.7). `Refs:` names only `TU-` ids. |
| `coverage` | Loom's crates and classes. Unchanged otherwise. |
| `ui` | New (R-j): `pnpm install --frozen-lockfile`, `tsc --noEmit`, `eslint`, `vitest run`, in `web/`. |
| `secrets` | New (R-h): `gitleaks` over the changes of the PR (CI) or since the merge base (local). Runs in `gate`. |
| `approval` | Unchanged, label-based (`needs-jann`, `approved`, fingerprint). P07 replaces it with `loom-approval`. |
| `github` | Loom's required checks and labels (no `windows`/`macos`). |

Everything else is ported as it is, including the guard lints, the `lints` code reading,
the verdict table and the CI context handling, which keeps PR text out of shell commands.

### 5.5 `web/`

React with Vite and TypeScript (strict), ESLint (flat config, TypeScript rules), and Vitest with
one smoke test of the root component (`// AC: TU-00.1`). `package.json` is private, uses pnpm
with a committed `pnpm-lock.yaml`, and pins the Node major in `engines` and `.nvmrc`. No UI
library or design system yet: that is P14.

### 5.6 `deny.toml`

Licences allowed: MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause,
ISC, Zlib, Unicode-3.0, CC0-1.0, MPL-2.0. Copyleft licences beyond MPL-2.0 are refused, so Loom
stays MIT-clean. Advisories: deny, and every ignore needs a reason. Sources: crates.io only.

### 5.7 CI

| Workflow | Job (= required check) | Runs | Notes |
|---|---|---|---|
| `ci` | `gate` | `cargo xtask gate --skip test,coverage` | saves the xtask cache on `main` |
| `ci` | `test` | `cargo xtask gate --only test` (nextest, doc tests, slow tests) | PostgreSQL service |
| `ci` | `coverage` | `cargo xtask gate --only coverage`, baseline cached by commit | PostgreSQL service |
| `ci` | `ui` | `cargo xtask gate --only ui` | not needed unless `web/` or the UI toolchain changed |
| `pr` | `pr` | `pr-title` and `pr-body`, with the cached xtask | `edited` events too |
| `approval` | `approval` | the editor's approval, from the base commit | labels and fingerprint |
| `settings` | `settings` | `cargo xtask github check`, weekly and on demand | not required |

- The `ci` jobs run side by side, each applying the docs-only rule through the gate's runner. A
  check that isn't needed reports "not needed" in seconds, so every required check always
  reports.
- Pull requests restore caches but never save them; only `main` saves. Concurrency cancels an
  older run of the same pull request.
- `run:` steps contain no `${{ }}` expressions (ported rule); xtask reads the event payload.
- **Speed budgets on a warm cache**, measured on the P00 PR and recorded in the trial record: a
  code pull request's `gate`, `test` and `coverage` each within 3 minutes of the run's start;
  `pr` and `approval` within 20 seconds; a docs-only pull request's required jobs within
  1 minute. They're measured on P01's PR (R-l); P00 records its cold-run times.

### 5.8 Documents

- **`docs/gate.md`**: the editor's structure (checks, lints, coverage, AC tags, protected paths,
  merging, the docs-only rule with its per-check reasons), with Loom's lists.
- **`CONTRIBUTING.md`**: branches, Conventional Commits (types as the editor's; scopes `core`,
  `plan`, `store`, `run`, `gate`, `context`, `api`, `cli`, `web`, `xtask`, `spec`, `docs`, `ci`),
  footers `Refs:`, `Phase:` and the Claude trailers, PR rules, tests.
- **PR template**: the editor's sections (Phase, Acceptance criteria, Final review, Rulings, Gate,
  Protected paths), plus a **Trial** section linking the phase's trial record.

## 6. How each AC is proven

| AC | Proof |
|---|---|
| TU-00.1 | A workspace test asserts that every crate of T1 exists with crate docs and inherits `[workspace.package]`; the binary's smoke test; the `web/` Vitest smoke test; the CI run of the P00 PR. |
| TU-00.2 | `xtask` tests that the required contexts in `gate/ruleset-main.json` equal the job names of the workflows, and that `docs/gate.md` lists exactly those checks; `cargo xtask github check` passes against the live repo once Jann has applied the settings. |
| TU-00.3 | The ported test that `cargo xtask gate` and the CI jobs call the same check functions with the same inputs, plus the CI run compared with a local run on the same commit. |
| TU-00.4 | The editor's tests for `ac-trace`, `pr-title`, `commit-msg`, `lints` and `approval`, ported with their fixtures and adapted to Loom's ids, branches and scopes. Every adaptation of §5.4 has a test of its own. |

## 7. Interfaces other phases use

- `cargo xtask gate` and every `cargo xtask <check>`; `--only` and `--skip`.
- `LOOM_TEST_DATABASE_URL`: set in the `test` and `coverage` jobs to the service database. P02
  adds how it's set locally.
- `ac-trace`'s rule for phases in other repos (P04).
- The trial record section of the PR template.

## 8. Risks

- **Porting carries editor-specific assumptions** (Windows paths, the engine, releases) into
  Loom. Mitigation: every adaptation listed in §5.4, the reviews read the port against this list,
  and unused code is removed rather than left dormant (directive 19).
- **The first cold-cache run is slow.** Budgets are measured on a warm cache; the cold numbers
  go into the trial record too.
- **Settings can't be applied by Claude.** `cargo xtask github apply` needs Jann's agreement and
  admin rights, so TU-00.2's live check passes only after Jann runs it.

## 9. Left to the builder

The exact tool versions (current releases at build time); how the port is split into commits
(the unchanged copy first); the Node major version; timeouts, measured on a cold cache.
