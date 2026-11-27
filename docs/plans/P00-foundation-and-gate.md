# P00 · Foundation and gate — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to
> implement this plan task-by-task, in a fresh session (`CLAUDE.md`). Steps use checkbox
> (`- [ ]`) syntax for tracking.

**Goal:** Loom's empty, documented workspace and `web/`, with the gate ported from theos-editor:
`cargo xtask gate` locally and four side-by-side CI jobs plus `pr` and `approval`, giving the
same verdicts.

**Architecture:** The editor's `xtask/` and `gate/` at `8b1b3d5` are copied unchanged, then
adapted one concern per task, so the PR shows every adaptation as a diff. The editor's P50
(Leaner CI) is built on the way: `gate --only/--skip`, a docs-only rule decided inside xtask, a
cached xtask for the light jobs. New parts: `ui`, `secrets`, and `ac-trace` reading a phase's
scope from its phase spec.

**Tech Stack:** Rust 1.99.1 (edition 2024), `clap`, `serde`/`toml`/`serde_json`, `globset`,
`syn`, `pulldown-cmark`, `git` and `gh` CLIs, cargo-nextest, cargo-llvm-cov, cargo-deny,
gitleaks, GitHub Actions; `web/`: React, Vite, TypeScript, ESLint, Vitest, pnpm.

**Spec:** `docs/specs/phases/P00-foundation-and-gate.md` (approved 27 Nov 2026). Decisions
0001–0005. The editor's P50 spec (`../theos-editor/docs/specs/phases/P50-leaner-ci.md` at
`8b1b3d5`) §5 is the design of `--only`/`--skip`, the docs-only rule and the caches; read it
before Tasks 9, 13 and 15.

**The editor's source.** `../theos-editor` (next to the main checkout of this repository) at
commit `8b1b3d5`. From a worktree, set it once per shell:

```bash
EDITOR_REPO="$(git rev-parse --path-format=absolute --git-common-dir)/../../theos-editor"
git -C "$EDITOR_REPO" cat-file -e 8b1b3d5^{commit} && echo ok
```

Read the editor's code with `git -C "$EDITOR_REPO" show 8b1b3d5:<path>`, never from its working
tree, which may have moved on.

---

## Files and areas P00 touches

Almost everything here is protected (spec R-k): the PR waits for Jann.

| Area | Paths | Tasks |
|---|---|---|
| Workspace | `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/config.toml`, `.gitattributes`, `.gitignore`, `clippy.toml`, `.config/nextest.toml`, `deny.toml` | 1, 2, 6 |
| Crates of T1 | `crates/{loom-core,loom-plan,loom-store,loom-run,loom-gate,loom-context,loom-api,loom}/` | 2 |
| The gate's code | `xtask/**` (ported; new modules `changes`, `code_paths`, `need`, `checks/secrets`, `checks/ui`, `checks/ac_trace/phase_spec`) | 1, 3–15 |
| The gate's data | `gate/**`: `lints.toml`, `clippy.toml`, `nextest.toml`, `thresholds.toml`, `tools.toml`, `cfg-names.toml`, `code-paths.toml`, `ruleset-main.json`, `repo-settings.toml` | 1, 6, 8, 10, 13, 14 |
| Repo contract | `.loom/protected.toml`, `.loom/checks.toml` | 7, 14 |
| UI | `web/**` | 12 |
| CI | `.github/workflows/{ci,pr,approval,settings}.yml`, `.github/actions/**`, `.github/pull_request_template.md` | 1, 14, 15 |
| Documents | `docs/gate.md`, `CONTRIBUTING.md`, `xtask/README.md` | 1, 14 |
| Trial | `docs/trial/P00-foundation-and-gate.md` | 16 |

Not touched: `CLAUDE.md`, `.claude/**`, `docs/specs/**`, `docs/decisions/**` (except a
decision Jann takes during implementation), `README.md`.

## Global Constraints (umbrella §8, verbatim)

- **T1** Rust stable, pinned, edition 2024; `unsafe_code` forbidden; `Cargo.lock` committed;
  builds `--locked`. Crates: `loom-core` (domain, state machines), `loom-plan` (planning repo
  reader and validation), `loom-store` (PostgreSQL through `sqlx`, migrations with `sqlx migrate`), `loom-run` (containers,
  worktrees, Claude Code runs), `loom-gate` (contract checks and audit), `loom-context`
  (index, briefings, MCP tools), `loom-api` (Axum) as libraries; `loom` (server and CLI) and
  `xtask` as binaries. The UI lives in `web/` (React, TypeScript, pnpm).
- **T2** One user, one subscription. Loom is Jann's personal tool and is never offered to
  others with his Claude login.
- **T3** Models run only through the Claude Code CLI in headless mode (`claude -p` with streamed
  JSON output), signed in with Jann's personal plan in the run containers. No direct API calls
  from Loom. Switching to an API key later is a config change, not a code change.
- **T4** Loom runs in a dedicated VM on the home server (not an LXC, so rootless nested
  containers and per-container egress filtering work). Every worker and reviewer runs in its
  own rootless container with its own git worktree. Inside a run, subagents share that worktree
  only when they run one after another; subagents that write in parallel each get their own
  worktree and branch, merged back one at a time. Network only to GitHub, the package
  registries the repo declares, and Anthropic. No access to the home LAN. Credentials: a
  fine-grained GitHub token scoped to the project's repos (contents and pull requests only).
  Commits and PRs appear under Jann's GitHub identity, as in theos-editor.
- **T4a GitHub App.** A Loom GitHub App, installed on the project's repos, is the only identity
  that posts `loom-review`, `loom-contract` and `loom-approval`. Branch protection pins these
  required checks to the App as their source, so nothing posted with Jann's token, and so
  nothing a worker posts, can satisfy them. The App never commits and never opens PRs. Its
  private key lives only in Loom's secret store, never in a container.
- **T4b Database.** Loom's state lives in PostgreSQL on Jann's dedicated database VM, in its own
  database and role. Only Loom's service reaches it. Worker containers never do, and the
  database's credentials never enter a container. Tests and CI use a throwaway PostgreSQL (a CI
  service container, or one started by `cargo xtask`), never the real database.
- **T5** Git only through the `git` command line; GitHub through `gh` or its REST API.
- **T6** All state is in git and PostgreSQL. Restarting Loom at any moment loses at most the
  running steps, which resume or retry from their last checkpoint.
- **T7** Library errors are typed (`thiserror`); `anyhow` only in binaries; logging with
  `tracing`. Every run's raw stream is kept for 30 days.
- **T8** Single sign-in for the cockpit (a passkey or password with a session cookie). Never
  exposed to the internet without the VPN or a reverse proxy in front. Secrets only in the
  server's secret store or environment, never in the planning repo.
- **T9** No telemetry. Embeddings and the index run locally.
- **T10** Escalations never contain internal numbering (§5.4).
- **T11** No worker, reviewer or decider run weakens a gate or posts a Loom check (only the App
  does, T4a). Loom never merges a PR that touches a protected path without `loom-approval`, and
  never edits Jann's planning files. Moves Jann makes on the board are his own edits (§3.2).
- **T12** Every AC below has at least one test tagged with its id. Loom's own repo meets the
  repo contract (§6.2), with theos-editor's gate as the model.
- **T13** The engineering directives of theos-editor's `CLAUDE.md` (maintainability first, use
  what exists, change the approach when fixes keep leaking, the gate is not a fortress) apply to
  Loom's own build.

## Acceptance criteria (umbrella §9 P00, verbatim)

**TU-00 Workspace, CI and gate**
- TU-00.1 The workspace with the crates of T1 (empty, documented) and `web/` builds and tests in
  CI on Linux.
- TU-00.2 Every check of `docs/gate.md` is a required status check on `main`. Branch protection
  matches R3.
- TU-00.3 `cargo xtask gate` gives the same verdicts as CI.
- TU-00.4 `ac-trace`, `pr-title`, `commit-msg`, `lints` and `approval` behave as in
  theos-editor's gate, until P07 replaces `approval` with `loom-approval`.

Spec §2 reads "matches R3" as R3's parts P00 can build; D5 (decision 0003) reads "check" as a
required status check: `gate`, `test`, `coverage`, `ui`, `pr`, `approval`.

## Brief for every subagent

Put this in every implementer, reviewer and fix brief:

- `CLAUDE.md` binds you: its hard rules (never weaken a check, never `#[allow]`, no
  `#[ignore]`/`#[expect]` without a reason, never `--no-verify`, never claim a check passed
  without running it and reading its output) and its six engineering directives.
- You port; you don't redesign. Keep the editor's code, names and comments unless this task
  says otherwise. Every change from the editor is in this plan or is a new `Ruling:` line you
  report back.
- D6 (decision 0004): the local gate runs on Windows and Linux. Keep the editor's Windows
  handling (`\` in paths, case-insensitive names, `cfg(windows)`).
- In code, `Check` keeps the editor's meaning: an xtask subcommand. In `docs/gate.md` that is a
  *part*; a *check* is a required status check (D5).
- Commits: Conventional Commits, scopes of Task 3's `SCOPES`, footer `Phase: P00`, trailer
  `Co-Authored-By: <your model> <noreply@anthropic.com>`. Author is Jann (the repository's
  `user.email` is set to his noreply address; don't change it).
- Between Task 2 and Task 15 some `xtask` test targets are red because the files they read are
  adapted later. A task turns the targets it names green and never turns a green target red.
  From Task 15 on, the whole suite is green.
- A task that changes a list a document mirrors, where a test keeps the two equal (the scopes and
  `CONTRIBUTING.md` §2, the protected paths and `docs/gate.md` §5, or any other such pair), also
  changes that list in the document, so the test stays green. Task 14 writes the rest of the
  documents. (Plan change allowed by Jann during implementation, 27 Nov 2026.)

## Rulings (Claude's calls; listed in the PR)

- Ruling: the first commit also copies the other editor files the port adapts (`.github/**`,
  `docs/gate.md`, `CONTRIBUTING.md`, `deny.toml`, `clippy.toml`, `.config/nextest.toml`,
  `.cargo/config.toml`, `rust-toolchain.toml`, `.gitattributes`) — every adaptation then shows
  as a diff, not only those of `xtask/` and `gate/` — a noisier first commit.
- Ruling: the root `Cargo.toml` and `Cargo.lock` are written new, not copied — Loom's members and
  dependencies share nothing with the editor's but the lint table — none.
- Ruling: linking uses the toolchain's default `rust-lld` instead of `mold` with `clang` (spec
  §5.1, §5.2) — `rust-lld` is the default linker of `x86_64-unknown-linux-gnu` since Rust 1.90,
  so it needs no install step and no config (directive 2); `mold` gains little over it — a few
  seconds of link time per job; `mold` can come in a `gate/` PR. Approved by Jann with the
  plan (decision 0005); the spec says so (D7).
- Ruling: one `umbrella::is_ac_id` (`TU-` + two digits + `.` + a number) used by the umbrella
  parser, `ac-trace` and `commit-msg` — one rule in one place (directive 1) — none.
- Ruling: the editor's AC tags in ported tests are retagged by behaviour: workspace shape → TU-00.1;
  ruleset, settings, `docs/gate.md` and `.loom/checks.toml` equality → TU-00.2; the gate and CI
  running the same parts with the same inputs → TU-00.3; `ac-trace`, `pr-title`, `commit-msg`,
  `lints`, `approval` → TU-00.4. Tags of parts no TU-00 criterion names (`pr-body` CE-08.7,
  `coverage` CE-08.8, `engine-free`, `platforms`) are dropped; test-data ids (`CE-90.1`,
  `CE-99.9`, …) become `TU-90.1`, `TU-99.9`, … — ac-trace refuses unknown ids, and a tag must say
  what the test proves — some tests carry no tag (T12 asks a test per AC, not a tag per test).
- Ruling: a phase spec's §2 scope is every list item under the level-two heading starting `2.`
  whose first word is an AC id; one spec file per phase (`docs/specs/phases/PNN-*.md`); every id
  listed must belong to that phase in the umbrella, and the list must not be empty — R-i made
  precise; a wrong scope must fail loudly — none.
- Ruling: the umbrella's phase table has no `Repo` column; a phase is built elsewhere when its
  heading ends `(repo \`owner/name\`)` (P04), otherwise in `theos-loom` — spec §5.4 — none.
- Ruling: code keeps the word `Check` for an xtask subcommand; only the docs say *part* (D5) —
  renaming 27,000 ported lines buys nothing — two words for one thing between code and docs.
- Ruling: a part that isn't needed gets a passing verdict with a `not_needed` reason, shown as
  `skip` in the table and *not needed* in the summary — P50 §5.3 asks it to be visible, never a
  silent pass — none.
- Ruling: `secrets` always runs, like `commit-msg`, also on a docs-only change — a doc can hold a
  secret; P50's rule only knew `commit-msg` — a few seconds on docs-only pull requests.
- Ruling: `tools` checks only the pins of the parts that will run — otherwise every job, and a
  docs-only run, would need Node, pnpm and gitleaks — none; an unused tool can't change a verdict.
- Ruling: Node is pinned by major version, pnpm and gitleaks exactly; `web/.nvmrc` and
  `web/package.json` (`engines.node`, `packageManager`) must equal the pins, which `tools`
  checks — Node's minor releases don't change tsc, ESLint or Vitest verdicts, which pnpm's lock
  fixes; one source of truth in `gate/tools.toml` — a verdict that differs between two Node 24
  releases.
- Ruling: there is no slow-test split: `test` runs every test, locally and in CI; the `slow-tests`
  part and the nextest profile `slow` go; `CONTRIBUTING.md` keeps the `slow_` name rule for a
  later split — spec §5.4 ("slow tests run inside `test`"); Loom has none — a slow `test` job
  once Loom has minutes-long tests, fixed by a `gate/` PR.
- Ruling: `pr-title` and `pr-body` run in one call, `cargo xtask pr`, through the gate's runner —
  one table, one summary, both always run (P50 §10 left this open) — none.
- Ruling: `ui` runs `pnpm exec <tool>` with fixed arguments, never a `package.json` script, and
  finds `pnpm` with the `which` crate — scripts are unprotected text that could make a part pass
  without running it; on Windows `pnpm` is a `.cmd` shim that `std::process::Command` doesn't
  find by name (D6) — none.
- Ruling: protected paths also include `.gitleaksignore`, `.gitleaks.toml` and
  `web/vite.config.*` — an ignore entry or a Vitest `exclude` silences a part as surely as an
  `#[allow]`; spec §5.3 missed them — three more files that wait for Jann.
- Ruling: `secrets` scans the commits of the range (the PR's, the push's, or the branch's since
  the merge base with `origin/main`), not the working tree; without a range (manual run) the
  whole history — the verdict then depends only on the commits, so local and CI agree on the
  same commit — a secret in an uncommitted file is found when it is committed.
- Ruling: CI installs gitleaks with `taiki-e/install-action` if that action's tool list has
  gitleaks at the pinned version; otherwise a step of the setup action downloads the release
  archive and checks it against the release's checksums file — directive 2 — none.
- Ruling: the `ui` job runs the cached xtask like `pr` and `approval`, with no Rust build cache —
  it builds no Rust of its own — on a cache miss it builds xtask (about a minute).
- Ruling: the PostgreSQL service runs with `POSTGRES_HOST_AUTH_METHOD: trust` and
  `LOOM_TEST_DATABASE_URL=postgres://postgres@localhost:5432/loom_test` — no password, so
  nothing in the repo looks like a credential (R10) — none; the service is reachable only from
  its job.
- Ruling: `.loom/checks.toml` is a list of `[[check]]` tables with `name`, `command` and `parts` —
  the least R2 needs; P06 owns the full format — P06 may reshape it.
- Ruling: `approval` reads its changed paths through the new shared reader (`changes`) — P50 §3
  — none.
- Ruling: `gate/code-paths.toml` doesn't list `docs/trial/**` separately: `docs/**` covers it
  (spec §5.3 "adds") — none. Always-code adds `docs/specs/phases/**`, because `ac-trace` reads
  phase specs (R-i) — P50's rule: every path a part reads is code — none.
- Ruling: the `ui` list is `web/**`, `gate/tools.toml`, `gate/code-paths.toml`, `xtask/**` and
  `.github/**` — what can change the `ui` verdict besides the UI itself — `ui` runs on some pull
  requests that can't affect it.
- Ruling: `web/` starts from Vite's `react-ts` template, trimmed; the smoke test uses
  `@testing-library/react` with `jsdom` — directive 2 — none.
- Ruling: crate READMEs are `README.md`, as in the editor; `CLAUDE.md`'s "README files are named
  README" is about the name, not the extension — none.
- Ruling: `[profile.dev.package."*"] opt-level = 1` stays from the editor — faster tests once
  dependencies arrive — none.
- Ruling: timeouts are three times the cold run, rounded up to 5 minutes, at least 10 —
  spec §9 leaves them to the build — a job cut off on a very slow runner day.
- Ruling: two probe pull requests against the phase branch prove the docs-only rule and the `ui`
  list in CI; they are closed unmerged — P50 §5.9 did the same — two short-lived PRs.

## Review Focus

1. **A docs-only pull request that adds a secret** to a doc: `secrets` runs and fails, every
   other part reports *not needed*. Test: Task 13 `a_docs_only_change_runs_only_commit_msg_and_secrets`.
2. **On Windows, pnpm is `pnpm.cmd`** (Corepack's shim): `cargo xtask gate --only ui` finds and
   runs it, and changed paths come back with `/`. Task 12 Step 6 runs it on Windows; Task 13
   `changed_paths_use_slashes` pins the paths.
3. **A local docs-only branch with an uncommitted or untracked `.rs` file:** every part runs.
   Test: Task 13 `locally_uncommitted_and_untracked_files_count`.
4. **A pull request that only edits a phase spec** (to narrow its §2): it is code (every part
   runs) and protected (waits for Jann). Tests: Task 13
   `the_real_lists_classify_every_input_a_part_reads`, Task 7 `the_real_rules_protect_the_gate`.
5. **`gate --only` or `--skip` with a typo or a name that isn't a gate part** (`tset`,
   `pr-title`, `tools`): exit code 2 naming it, never a green run that ran nothing. Test: Task 9
   `only_and_skip_refuse_a_name_that_is_not_a_gate_part`.

---

## Task 1: Copy the editor's gate unchanged

**ACs:** none (the base every later diff is read against).

**Files:**
- Create, byte for byte from the editor at `8b1b3d5`: `xtask/**`, `gate/**`, `.github/**`,
  `docs/gate.md`, `CONTRIBUTING.md`, `deny.toml`, `clippy.toml`, `.config/nextest.toml`,
  `.cargo/config.toml`, `rust-toolchain.toml`, `.gitattributes`

**Interfaces:**
- Produces: the editor's tree at these paths, for Tasks 2–15 to adapt.

- [ ] **Step 1: Extract the paths from the editor's commit**

```bash
PATHS="xtask gate .github docs/gate.md CONTRIBUTING.md deny.toml clippy.toml .config .cargo rust-toolchain.toml .gitattributes"
git -C "$EDITOR_REPO" archive 8b1b3d5 -- $PATHS | tar -x -C .
git add -- $PATHS
```

- [ ] **Step 2: Commit**

```bash
git commit -F - <<'EOF'
chore(xtask): copy the gate of theos-editor at 8b1b3d5

An unchanged copy of xtask/, gate/ and the editor files the port
adapts, so every adaptation in P00 shows as a diff (spec R-a). It
does not build yet; the workspace comes next.

Phase: P00
Co-Authored-By: <model> <noreply@anthropic.com>
EOF
```

- [ ] **Step 3: Verify the copy is exact**

Run: `diff <(git -C "$EDITOR_REPO" ls-tree -r 8b1b3d5 -- $PATHS) <(git ls-tree -r HEAD -- $PATHS) && echo identical`
Expected: `identical` (same modes, blob hashes and paths).

---

## Task 2: Workspace, crates and licences

**ACs:** TU-00.1

**Files:**
- Create: `Cargo.toml`, `Cargo.lock`, `crates/<crate>/{Cargo.toml,README.md,src/lib.rs}` for the
  seven libraries, `crates/loom/{Cargo.toml,README.md,src/main.rs,tests/smoke.rs}`
- Modify: `.gitignore` (add `node_modules/`, `web/dist/`), `xtask/Cargo.toml` (add
  `license.workspace = true`), `deny.toml`, `xtask/tests/workspace.rs`

**Interfaces:**
- Produces: a workspace where `cargo build --workspace --locked` and `cargo xtask --help` work.

- [ ] **Step 1: Adapt `xtask/tests/workspace.rs`** (`// AC: TU-00.1` on each test)

`workspace_members_are_the_t1_crates` (renamed) expects exactly, sorted:
`loom-api@crates/loom-api`, `loom-context@crates/loom-context`, `loom-core@crates/loom-core`,
`loom-gate@crates/loom-gate`, `loom-plan@crates/loom-plan`, `loom-run@crates/loom-run`,
`loom-store@crates/loom-store`, `loom@crates/loom`, `xtask@xtask`.
`every_member_has_readme_and_crate_docs` stays. `every_member_inherits_the_workspace_package`
(extends the editor's version test): every member declares `version`, `edition`,
`rust-version`, `publish`, `repository` and `license` as `{ workspace = true }`, and the root
`[workspace.package]` has `license = "MIT"`.

- [ ] **Step 2: Write the smoke test `crates/loom/tests/smoke.rs`**

```rust
// AC: TU-00.1
#[test]
fn the_binary_runs_and_exits_zero() -> Result<(), Box<dyn std::error::Error>> {
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_loom")).status()?;
    assert!(status.success(), "loom exited with {status}");
    Ok(())
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `cargo nextest run -p xtask --test workspace`
Expected: FAIL (no workspace root yet; then wrong members).

- [ ] **Step 4: Write the workspace**

Root `Cargo.toml`: `[workspace]` `resolver = "3"`, `members = ["crates/*", "xtask"]`;
`[workspace.package]` `version = "0.0.0"`, `edition = "2024"`, `rust-version = "1.99.1"`,
`publish = false`, `repository = "https://github.com/FPGSchiba/theos-loom"`, `license = "MIT"`;
`[workspace.lints.*]` copied from the editor's root `Cargo.toml` (it equals `gate/lints.toml`);
`[workspace.dependencies]` with exactly the crates `xtask/Cargo.toml` uses, at the editor's
versions; `[profile.dev.package."*"] opt-level = 1`. No `theos-maps` profile.

Each crate: `[package]` with every `[workspace.package]` key as `.workspace = true`, `[lints]
workspace = true`, no dependencies. `src/lib.rs` (or `src/main.rs` for `loom`) starts with `//!`
crate docs that state the crate's role in T1's words (`loom-core`: domain, state machines;
`loom-plan`: planning repo reader and validation; `loom-store`: PostgreSQL through `sqlx`,
migrations with `sqlx migrate`; `loom-run`: containers, worktrees, Claude Code runs;
`loom-gate`: contract checks and audit; `loom-context`: index, briefings, MCP tools;
`loom-api`: the Axum API; `loom`: the server and CLI) and that it is empty until its phase. The
binary is `fn main() {}` with a doc comment. `README.md` says the same in two or three lines.

`.cargo/config.toml` stays the editor's (the `xtask` alias only; see the linker ruling).

- [ ] **Step 5: Adapt `deny.toml`** (spec §5.6)

Licences allowed, exactly: `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`,
`BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`, `CC0-1.0`, `MPL-2.0`. Remove the
G20 `[bans] deny` list and the engine's `allow-git`; `[sources]` allows crates.io only. Keep the
rest (advisories deny yanked, ignores need a reason). If `cargo deny` then refuses a licence
outside this list, **stop and ask Jann**: the spec decides the list.

- [ ] **Step 6: Build, test, check**

Run: `cargo build --workspace --locked` (after one `cargo generate-lockfile`), then
`cargo nextest run -p xtask --test workspace` and `cargo nextest run -p loom`, then
`cargo deny --locked check`, then `cargo xtask --help`.
Expected: builds; both test runs PASS; deny `ok`; the help lists the subcommands.

- [ ] **Step 7: Commit** — `build: the workspace with the crates of T1` (footer and trailer as in the brief).

---

## Task 3: Loom's ids, umbrella and scopes

**ACs:** TU-00.4

**Files:**
- Modify: `xtask/src/umbrella.rs`, `xtask/src/conventional.rs`,
  `xtask/src/checks/ac_trace/mod.rs` (`is_tag_id`), `xtask/src/checks/commit_msg.rs`, doc
  comments in `xtask/src/markdown.rs` and `xtask/src/report.rs`, the scope list of
  `CONTRIBUTING.md` §2 (the mirrored-list rule of the brief)
- Modify: every `xtask/tests/fixtures/ac-trace/*/docs/specs/00-umbrella.md` (Loom's format) and
  every `// AC:` tag and test-data id under `xtask/` (the retagging ruling)

**Interfaces:**
- Produces: `umbrella::OWN_REPO: &str = "theos-loom"`; `pub fn umbrella::is_ac_id(id: &str) -> bool`;
  `PhaseInfo { id, name, repo, acs }` as before, with `repo` from the heading;
  `conventional::SCOPES: [&str; 13]`. (`BranchKind` loses `Release` in Task 5, not here.
  (Plan change allowed by Jann during implementation, 27 Nov 2026.))

- [ ] **Step 1: Write the failing tests** (in `umbrella.rs`, `// AC: TU-00.4`)

- `ids_are_tu_two_digits_dot_number`: true for `TU-00.1`, `TU-14.12`; false for `CE-08.1`,
  `TU-08C.1`, `TU-0.1`, `TU-000.1`, `TU-00.`, `TU-00.x`, `tu-00.1`, `TU-00.1.2`.
- `loom_umbrella_reads_every_phase` over the real `docs/specs/00-umbrella.md`: 21 phases P00–P20;
  `phase(P00).acs == ["TU-00.1", "TU-00.2", "TU-00.3", "TU-00.4"]`; `phase(P00).repo ==
  "theos-loom"`; `phase(P04).repo == "FPGSchiba/bmad-method"` and its name is `Workflow kit`;
  `all_ids()` contains `TU-08.2`.
- `a_heading_repo_suffix_names_the_repo`: `### P07 · X (repo \`o/r\`)` gives repo `o/r`, name `X`.
- `a_table_without_name_column_is_an_error` (the editor's table test, with `Phase | Name` as the
  required columns).

- [ ] **Step 2: Run** `cargo nextest run -p xtask -E 'test(/umbrella/)'` — Expected: FAIL.

- [ ] **Step 3: Adapt the parser, scopes and ids**

`umbrella.rs`: ids are `TU-` + exactly two digits + `.` + one or more digits; the phase table
needs `Phase` and `Name` columns; the repo is the heading's `(repo \`…\`)` suffix, else
`OWN_REPO`. The editor's checks for doubled phases, list items that start `- TU-` but aren't an
id, and lone CRs stay. `ac_trace::is_tag_id` and `commit_msg`'s `Refs:` check call
`umbrella::is_ac_id`.
`conventional.rs`: `SCOPES = ["core", "plan", "store", "run", "gate", "context", "api", "cli",
"web", "xtask", "spec", "docs", "ci"]`; `TYPES` unchanged. `CONTRIBUTING.md` §2 lists the same
scopes, so the editor's test that keeps the two equal (`xtask/tests/commit_msg.rs`) stays green.

Fixture umbrellas: drop the `Repo` column, write the repo of a phase built elsewhere as the
heading suffix (`other-repo-phase`: `(repo \`FPGSchiba/bmad-method\`)`), `CE-` ids → `TU-`.
Retag every `// AC:` comment under `xtask/` by the retagging ruling.

- [ ] **Step 4: Run** `cargo nextest run -p xtask -E 'test(/umbrella|branch|conventional/)'` and
`cargo nextest run -p xtask --test commit_msg -E 'test(types_and_scopes_equal_contributing_md_section_2)'`
— Expected: PASS. Then `git grep -n 'CE-' -- xtask` — Expected: no output.

- [ ] **Step 5: Commit** — `refactor(xtask): read Loom's umbrella, ids and scopes`.

---

## Task 4: `ac-trace` for Loom

**ACs:** TU-00.4

**Files:**
- Create: `xtask/src/checks/ac_trace/phase_spec.rs`
- Modify: `xtask/src/checks/ac_trace/mod.rs`, `xtask/tests/ac_trace.rs`

**Interfaces:**
- Consumes: `umbrella::{Umbrella, PhaseInfo, OWN_REPO, is_ac_id}`, `markdown::{sections, list_items}`.
- Produces: `pub const PHASES_DIR: &str = "docs/specs/phases";`
  `pub enum Scope { Umbrella, PhaseSpec { file: String, acs: Vec<String> } }`;
  `pub fn scope(root: &Path, umbrella: &Umbrella, phase: Phase) -> Result<Scope>`;
  `ac_trace::check(umbrella, phase, scope: &Scope, tags, junit, out) -> Verdict`.

- [ ] **Step 1: Write the failing tests** (`// AC: TU-00.4`)

In `phase_spec.rs`, over temp directories with a fixture umbrella whose P90 has `TU-90.1`,
`TU-90.2`, `TU-90.3`:
- `a_phase_spec_lists_the_part_a_phase_builds`: `docs/specs/phases/P90-x.md` with `## 2. Acceptance
  criteria` listing `- TU-90.1 …` and `- TU-90.2 …` → `PhaseSpec { file: "docs/specs/phases/P90-x.md", acs: ["TU-90.1", "TU-90.2"] }`.
- `without_a_phase_spec_the_umbrella_decides` → `Scope::Umbrella`.
- `a_phase_spec_naming_a_criterion_of_another_phase_is_an_error`: `Error::Config` naming the id
  and the file.
- `two_phase_specs_for_one_phase_are_an_error`.
- `a_phase_spec_without_a_criterion_in_section_2_is_an_error`.
- `p00_demands_its_four_criteria`: the real repository gives `PhaseSpec` with `TU-00.1`…`TU-00.4`.

In `mod.rs`: `a_phase_spec_narrows_the_criteria_demanded` (tags for 90.1 and 90.2 only → pass;
the table heading names the spec file); `typescript_files_are_tracked_outside_fixtures_node_modules_and_target`
extended with `web/src/App.test.tsx` (tracked) and `web/node_modules/x/a.tsx` (not).

- [ ] **Step 2: Run** `cargo nextest run -p xtask -E 'test(/ac_trace|phase_spec/)'` — Expected: FAIL.

- [ ] **Step 3: Implement**

`phase_spec::scope` globs `PHASES_DIR/PNN-*.md`, reads §2 with `markdown::sections(text, 2)`
(heading starts `2.`) and `markdown::list_items`, keeps items whose first word passes
`is_ac_id`. `ac_trace::run_phase` asks for the scope only for a phase built in `OWN_REPO`; the
criteria demanded are the scope's (or the phase's from the umbrella). `is_typescript` accepts
`ts` and `tsx`. `read_junit` reads only the report of the profile `ci` (CI) or `default`
(locally); the slow report and its tests go. The ported tests' fixture phase P90 stays.

- [ ] **Step 4: Run** the Step 2 command and `cargo nextest run -p xtask --test ac_trace` —
Expected: PASS.

- [ ] **Step 5: Commit** — `feat(xtask): take a phase's criteria from its phase spec`.

---

## Task 5: `pr-title` and `commit-msg` by Loom's rules

**ACs:** TU-00.4

**Files:**
- Modify: `xtask/src/branch.rs`, `xtask/src/checks/pr_title.rs`, `xtask/src/checks/commit_msg.rs`,
  `xtask/tests/pr_title.rs`, `xtask/tests/commit_msg.rs`, `xtask/tests/fixtures/events/pull_request.json`

**Interfaces:**
- Consumes: Task 3's `SCOPES`, `is_ac_id`.
- Produces: `BranchKind` without `Release` (moved here from Task 3, so the type and the code that
  matches on it go in one commit. (Plan change allowed by Jann during implementation, 27 Nov 2026.))

- [ ] **Step 1: Adapt the tests** (`// AC: TU-00.4`)

Remove `release_titles_pass` and `a_release_branch_needs_a_release_title`. Add
`a_release_title_is_no_title_here`: `chore(release): v0.1.0` fails on `phase/P01-x` (needs
`(P01)`) and on `release-plz-2026-11-01` (a branch without a rule). Keep every other title test
with Loom's scopes (`feat(core): x (P01)` passes; `feat(content): x (P01)` fails naming the
scopes). Commit tests: a `Refs: TU-00.1` footer passes, `Refs: CE-08.1` fails as unknown;
`known_acs_are_the_ids_of_the_umbrella_spec` checks `TU-00.1`, `TU-08.2` in and `TU-99.9` out.

- [ ] **Step 2: Run** `cargo nextest run -p xtask --test pr_title --test commit_msg` — Expected: FAIL.

- [ ] **Step 3: Implement** — `branch.rs`: remove `Release` (`release-plz-*` becomes `Other`; its
tests follow). Remove `is_release_title` and the `Release` arms; the suffix rules
stay the editor's: `phase/` and `spec/` → `(PNN)`, `gate/` → `(gate)`, `docs/` → `(docs)`.

- [ ] **Step 4: Run** the Step 2 command and `cargo nextest run -p xtask -E 'test(/pr_title|commit_msg|conventional|branch/)'` — Expected: PASS.

- [ ] **Step 5: Commit** — `feat(xtask): titles and commits by Loom's rules`.

---

## Task 6: Lint configuration

**ACs:** TU-00.4

**Files:**
- Modify: `gate/clippy.toml`, `clippy.toml`, `gate/cfg-names.toml`, `gate/nextest.toml`,
  `.config/nextest.toml`, `xtask/tests/fixtures/lints/clean/gate/*` (mirror the new files),
  `xtask/tests/lints.rs` and `xtask/src/checks/lints/**` (retagging done in Task 3; only
  expectations that name editor files change)

- [ ] **Step 1: Adapt the expectations** — `lints` tests that compare against the real `gate/`
files expect the new contents; the fixture `clean` mirrors them.

- [ ] **Step 2: Change the files**

`gate/clippy.toml` and its copy `clippy.toml`: only the five `allow-*-in-tests = true` lines and
a one-line comment; the G20 `disallowed-methods` go. `gate/cfg-names.toml`: `names = ["test",
"doc", "unix", "windows"]`; `keys` = the union of `rustc +1.99.1 --print cfg --target
x86_64-unknown-linux-gnu` and `--target x86_64-pc-windows-msvc` keys without
`debug_assertions` and `panic`; the comment names these two targets (D6) and the commands.
`gate/nextest.toml` and its copy `.config/nextest.toml`: profiles `default` and `ci`, each
`retries = 0`, `fail-fast = false`, `junit.path = "junit.xml"`, no `default-filter`; no `slow`
profile; the comment says `test` runs every test (the slow-test ruling).

- [ ] **Step 3: Run** `cargo nextest run -p xtask --test lints`,
`cargo nextest run -p xtask --lib -E 'test(/lints/)'` and `cargo xtask lints` — Expected: PASS,
and `pass  lints`.

- [ ] **Step 4: Commit** — `feat(gate): Loom's lint and test configuration`.

---

## Task 7: Protected paths in `.loom/protected.toml`

**ACs:** TU-00.4

**Files:**
- Move: `gate/protected-paths.toml` → `.loom/protected.toml` (`git mv`, then edit)
- Modify: `xtask/src/checks/approval/protected.rs` (`CONFIG`), `xtask/src/checks/approval/mod.rs`
  (messages naming the file), `xtask/tests/approval.rs`, the list of `docs/gate.md` §5 (the
  mirrored-list rule of the brief: the editor's `protected_paths_toml_matches_gate_md_section_5`
  keeps it equal to `.loom/protected.toml`; it is retagged `// AC: TU-00.4`) (Plan change allowed by Jann during implementation, 27 Nov 2026.)

**Interfaces:**
- Produces: `approval::protected::CONFIG: &str = ".loom/protected.toml"`.

- [ ] **Step 1: Write the failing tests** (`// AC: TU-00.4`, in `protected.rs`)

- `the_real_rules_protect_the_gate`: each of `CLAUDE.md`, `.claude/skills/x/SKILL.md`,
  `CONTRIBUTING.md`, `docs/gate.md`, `docs/specs/00-umbrella.md`, `docs/specs/phases/P01-x.md`,
  `docs/decisions/0005-x.md`, `.github/workflows/ci.yml`, `gate/tools.toml`, `xtask/src/lib.rs`,
  `deny.toml`, `rust-toolchain.toml`, `.cargo/config.toml`, `.loom/checks.toml`,
  `.loom/protected.toml`, `web/eslint.config.js`, `web/tsconfig.app.json`, `web/vite.config.ts`,
  `.gitleaksignore`, `.gitleaks.toml`, and `Docs/Gate.md` (case) is protected.
- `the_real_rules_leave_ordinary_code_alone`: `crates/loom-core/src/lib.rs`, `web/src/App.tsx`,
  `web/package.json`, `Cargo.lock`, `README.md`, `docs/plans/P01-x.md`, `docs/trial/P01-x.md`
  are not.
- `docs_protected_plans_and_trial_not` replaces the editor's `…_plans_and_status_not`.
- The `[[tables]]` rule for `Cargo.toml` `workspace.lints` stays, with its test.

- [ ] **Step 2: Run** `cargo nextest run -p xtask -E 'test(/protected|approval/)'` — Expected: FAIL.

- [ ] **Step 3: Write `.loom/protected.toml`**

`protected = ["CLAUDE.md", ".claude/**", "CONTRIBUTING.md", "docs/**", ".github/**", "gate/**",
"xtask/**", ".loom/**", "deny.toml", "rust-toolchain.toml", ".cargo/**", "web/eslint.config.*",
"web/tsconfig*.json", "web/vite.config.*", ".gitleaksignore", ".gitleaks.toml"]`,
`except = ["docs/plans/**", "docs/trial/**"]`, the `workspace.lints` table rule; the header
comment as the editor's, naming `.loom/protected.toml` and `docs/gate.md` §5.

- [ ] **Step 4: Run** the Step 2 command and `cargo nextest run -p xtask --test approval` — Expected: PASS.

- [ ] **Step 5: Commit** — `feat(xtask): protected paths in .loom/protected.toml`.

---

## Task 8: Drop what Loom doesn't build; Loom's coverage classes

**ACs:** TU-00.3

**Files:**
- Delete: `xtask/src/checks/engine_free.rs`, `xtask/src/checks/slow.rs`,
  `xtask/tests/engine_free.rs`, `xtask/tests/slow.rs`, `xtask/tests/fixtures/engine-free/`,
  `gate/engine-crates.toml`, `gate/ruleset-tags.json`, `.github/workflows/platforms.yml`
- Modify: `xtask/src/verdict.rs`, `xtask/src/gate.rs`, `xtask/src/main.rs`, `xtask/src/lib.rs`,
  `xtask/src/checks/mod.rs`, `xtask/src/checks/cargo_cmds.rs`, `xtask/src/github/settings.rs`,
  `gate/repo-settings.toml`, `gate/thresholds.toml`, coverage tests that name editor crates

**Interfaces:**
- Produces: `Check` without `EngineFree` and `SlowTests`; `GATE_ORDER` = `Tools, Fmt, Lints,
  CommitMsg, Clippy, Docs, Deny, Test, Coverage, AcTrace` (Tasks 11 and 12 add `Secrets` after
  `CommitMsg` and `Ui` before `AcTrace`); `cargo_cmds::test(ci: bool)` with every test.

- [ ] **Step 1: Adapt the tests**

`gate.rs` `placed!` without `EngineFree`/`SlowTests`; `gate_runs_every_ci_check_except_pr_only`
gets `// AC: TU-00.3`. `settings` tests: one ruleset (`main`), labels `needs-jann` and `approved`
only; delete `tag_ruleset_blocks_update_and_deletion`. Coverage tests use Loom's crates.

- [ ] **Step 2: Run** `cargo nextest run -p xtask --lib` — Expected: FAIL (compile errors first).

- [ ] **Step 3: Remove and adapt**

Delete the files above and every reference (subcommands, `run_check` arms, docs in `lib.rs`,
`WINDOWS`/`MACOS` labels, `RULESET_FILES = ["gate/ruleset-main.json"]`). `cargo_cmds::test`
keeps profiles `ci`/`default`, which now run every test; `slow_tests()` goes.
`gate/repo-settings.toml`: the `windows` and `macos` labels go. `gate/thresholds.toml`:
`max_drop = 1.0`, floors `library = 80.0`, `binary = 60.0`; `[crates]` the seven libraries and
`xtask` = `"library"`, `loom` = `"binary"`.

- [ ] **Step 4: Run** `cargo nextest run -p xtask --lib`, `cargo nextest run -p xtask --test coverage`,
`cargo clippy --workspace --all-targets --locked -- -D warnings` — Expected: PASS, no warnings.
Then `git grep -n -i 'engine-free\|slow-tests\|platforms\|release-plz' -- xtask gate` —
Expected: no output.

- [ ] **Step 5: Commit** — `refactor(xtask): drop what Loom does not build`.

---

## Task 9: Run a part of the gate: `--only`, `--skip`, *not needed*, `cargo xtask pr`

**ACs:** TU-00.3

**Files:**
- Modify: `xtask/src/verdict.rs`, `xtask/src/gate.rs`, `xtask/src/report.rs`, `xtask/src/main.rs`,
  `xtask/tests/cli.rs`

**Interfaces:**
- Produces:
  - `Verdict.not_needed: Option<String>`; `Verdict::not_needed(check: Check, reason: impl Into<String>) -> Verdict` (passes); `Verdict::is_not_needed(&self) -> bool`.
  - `Check::from_name(name: &str) -> Option<Check>`; `pub const ALL: [Check; N]`.
  - `pub enum Selection { All, Only(Vec<Check>), Skip(Vec<Check>) }`;
    `pub fn gate::selection(only: &[String], skip: &[String]) -> Result<Selection>`;
    `pub fn gate::selected(selection: &Selection) -> Vec<Check>` (`Tools` first, then the chosen
    checks in `GATE_ORDER`);
    `pub fn gate::run(root: &Path, ci: Option<&CiEvent>, selection: &Selection, out: &mut dyn Write) -> Result<Vec<Verdict>>`;
    `pub struct PrInputs { pub title: Option<String>, pub body_file: Option<PathBuf>, pub branch: Option<String> }`
    (`None`: from the event, as the editor's `pr-title`/`pr-body` do);
    `pub fn gate::run_pr(root: &Path, ci: Option<&CiEvent>, inputs: &PrInputs, out: &mut dyn Write) -> Result<Vec<Verdict>>`.
  - `pub fn report::emit_all(verdicts: &[Verdict], ci: bool, out: &mut dyn Write) -> Result<()>`
    (table; under CI also annotations and one summary).
  - CLI: `cargo xtask gate [--only a,b | --skip a,b]`; `cargo xtask pr [--title T --body-file F [--branch B]]`.

- [ ] **Step 1: Write the failing tests** (`// AC: TU-00.3`)

- `only_runs_tools_and_the_named_parts_in_gate_order`: `--only test,fmt` → `[Tools, Fmt, Test]`.
- `skip_runs_every_other_part`: `--skip test,coverage` → `GATE_ORDER` without them.
- `only_and_skip_refuse_a_name_that_is_not_a_gate_part`: each of `tset`, `pr-title`, `tools`,
  `github`, `approval` is an error whose text names it; `--only` with `--skip` is refused by
  clap (in `tests/cli.rs`, exit code 2).
- `a_not_needed_part_passes_and_shows_its_reason`: the table has `skip  fmt` and
  `  note: not needed: <reason>`; `summary_markdown` has `| \`fmt\` | not needed |`.
- `the_gate_writes_one_summary_with_every_part`: `emit_all` with a summary file → one `### xtask`
  table with a row per verdict.
- `pr_runs_title_and_body_and_reports_both`: a failing title still runs the body; both verdicts
  are in the table.

- [ ] **Step 2: Run** `cargo nextest run -p xtask -E 'test(/gate|report|cli/)'` — Expected: FAIL.

- [ ] **Step 3: Implement** — the filter and the `not_needed` state over the existing
`run_checks`; `Tools` always runs first and still stops the gate when it fails. `gate` writes
its table through `emit_all` (it already wrote the table; now also annotations and the
summary under CI). `pr` runs `PrTitle` then `PrBody` through `run_checks` with no `Tools`.
The `pr-title` and `pr-body` subcommands stay.

- [ ] **Step 4: Run** the Step 2 command — Expected: PASS. `cargo xtask gate --only fmt` prints
`pass  tools` and `pass  fmt`.

- [ ] **Step 5: Commit** — `feat(xtask): run a part of the gate with --only and --skip`.

---

## Task 10: Pinned tools for the parts that run

**ACs:** TU-00.3

**Files:**
- Modify: `xtask/src/tools.rs`, `gate/tools.toml`, `xtask/src/gate.rs`, `xtask/src/main.rs`

**Interfaces:**
- Produces: `pub enum Tool { CargoNextest, CargoDeny, CargoLlvmCov, Gitleaks, Node, Pnpm }`;
  `pub fn tools::needed_by(check: Check) -> &'static [Tool]` (`Test` → nextest; `Coverage` →
  llvm-cov, nextest; `Deny` → cargo-deny; every other → none; Tasks 11 and 12 add `Secrets` →
  gitleaks and `Ui` → node, pnpm); `pub fn tools::run_for(root: &Path, checks: &[Check], out: &mut dyn Write) -> Result<Verdict>`;
  CLI `cargo xtask tools [--for a,b] [--install | --print-install-spec]`.

- [ ] **Step 1: Write the failing tests**

- `pins_name_every_tool`: `gate/tools.toml` `[tools]` maps `cargo-nextest`, `cargo-deny`,
  `cargo-llvm-cov`, `gitleaks`, `node`, `pnpm`; any other name or a missing one is an
  `Error::Config`.
- `versions_are_read_from_each_tools_output`: `v24.19.0` (`node --version`) gives major `24`;
  `10.20.0` (`pnpm --version`) gives `10.20.0`; `gitleaks version 8.28.0` and `v8.28.0` give
  `8.28.0`; the cargo tools as before.
- `node_matches_its_pin_by_major`: pin `24`, installed `24.11.1` passes; `22.1.0` fails.
- `only_the_tools_of_the_parts_that_run_are_checked`: `run_for(&[Tools, Fmt])` checks none and
  passes with a note; `run_for(&[Test])` checks only nextest; `cargo xtask tools` without
  `--for` checks every pin.
- `the_install_spec_lists_only_cargo_installable_tools`: `--print-install-spec` prints
  `cargo-nextest@…,cargo-deny@…,cargo-llvm-cov@…` (and `gitleaks@…` only if the install-action
  ruling took that branch).

- [ ] **Step 2: Run** `cargo nextest run -p xtask -E 'test(/tools/)'` — Expected: FAIL.

- [ ] **Step 3: Implement** — `gate/tools.toml` gets `gitleaks`, `node` (a major, such as `"24"`)
and `pnpm` at their current releases (spec §9). `--install` installs the cargo tools as the
editor does and, for Node, pnpm and gitleaks, prints how (`corepack enable` and
`corepack prepare pnpm@<pin> --activate`; gitleaks: `winget install Gitleaks.Gitleaks` on
Windows, the release archive on Linux) instead of installing them. `gate::run` passes the parts
that will run to `tools::run_for` (Task 13 narrows that list to the needed parts).

- [ ] **Step 4: Install the local tools** (D6): pnpm through Corepack and gitleaks through
`winget`, at the pins. Run `cargo xtask tools` — Expected: `pass  tools`.

- [ ] **Step 5: Run** the Step 2 command — Expected: PASS.

- [ ] **Step 6: Commit** — `feat(xtask): pin Node, pnpm and gitleaks; check the tools a run needs`.

---

## Task 11: The `secrets` part

**ACs:** TU-00.3

**Files:**
- Create: `xtask/src/checks/secrets.rs`
- Modify: `xtask/src/checks/mod.rs`, `xtask/src/verdict.rs` (`Check::Secrets`, name `secrets`,
  in `GATE_ORDER` after `CommitMsg`), `xtask/src/gate.rs`, `xtask/src/main.rs`, `xtask/src/tools.rs`
  (`Secrets` → gitleaks)

**Interfaces:**
- Produces: `pub enum Range { Commits { from: String, to: String }, History }`;
  `pub fn secrets::range(git: &Git, ci: Option<&CiEvent>) -> Result<Range>`;
  `pub fn secrets::command(range: &Range, report: &Path) -> Vec<String>`;
  `pub fn secrets::findings(report_json: &str) -> Result<Vec<Finding>>`;
  `pub fn secrets::run(root: &Path, ci: Option<&CiEvent>, out: &mut dyn Write) -> Result<Verdict>`;
  subcommand `cargo xtask secrets`.

- [ ] **Step 1: Write the failing tests**

- `a_pull_request_scans_its_commits_since_the_merge_base`: `Range::Commits { from: <merge
  base>, to: <head sha> }`; a push scans `before..after`; a push whose `before` is all zeros,
  `workflow_dispatch` and a detached HEAD scan `History`; locally, `merge-base(HEAD,
  origin/main)..HEAD`.
- `the_command_scans_git_with_a_redacted_json_report`: `["git", "--log-opts=<from>..<to>",
  "--redact", "--no-banner", "--exit-code", "0", "--report-format", "json", "--report-path",
  <report>]` (exit code 0: the report decides, so a crash of gitleaks stays an error);
  `History` has no `--log-opts`.
- `each_leak_is_a_finding_without_the_secret`: a report with two entries (`RuleID`, `File`,
  `StartLine`, `Commit`, `Description`, `Secret: "REDACTED"`) gives two findings at `File:StartLine`
  naming the rule and the short commit and never the `Secret` field; `[]` gives none; text that
  isn't the report is an error.

- [ ] **Step 2: Run** `cargo nextest run -p xtask -E 'test(/secrets/)'` — Expected: FAIL.

- [ ] **Step 3: Implement** — run gitleaks with `std::process::Command` (an argument list, no
shell), writing the report to `target/xtask/gitleaks.json`. No test runs gitleaks itself: CI's
`test` job doesn't install it (Task 10); Step 5 proves the real run.

- [ ] **Step 4: Run** the Step 2 command — Expected: PASS.

- [ ] **Step 5: Prove a real leak is found** — on a scratch branch off the phase branch, commit a
file with a fake key that gitleaks' `generic-api-key` rule matches (generate it at the shell,
never paste one into the plan or the repo), run `cargo xtask secrets` — Expected: `FAIL  secrets`
with one finding; then delete the scratch branch (`git branch -D`). Run it on the phase branch —
Expected: `pass  secrets`.

- [ ] **Step 6: Commit** — `feat(xtask): scan the commits for secrets with gitleaks`.

---

## Task 12: The `ui` part and `web/`

**ACs:** TU-00.1, TU-00.3

**Files:**
- Create: `web/` from `pnpm create vite@<current> web --template react-ts`, trimmed to:
  `package.json`, `pnpm-lock.yaml`, `.nvmrc`, `index.html`, `tsconfig.json`,
  `tsconfig.app.json`, `tsconfig.node.json`, `eslint.config.js`, `vite.config.ts`,
  `src/main.tsx`, `src/App.tsx`, `src/App.test.tsx`, `README.md`
- Create: `xtask/src/checks/ui.rs`
- Modify: `xtask/src/checks/mod.rs`, `xtask/src/verdict.rs` (`Check::Ui`, name `ui`, in
  `GATE_ORDER` before `AcTrace`), `xtask/src/gate.rs`, `xtask/src/main.rs`, `xtask/src/tools.rs`
  (`Ui` → node, pnpm), `Cargo.toml` and `xtask/Cargo.toml` (`which`)

**Interfaces:**
- Produces: `pub const WEB_DIR: &str = "web";` `pub fn ui::commands() -> Vec<Vec<&'static str>>`;
  `pub fn ui::run(root: &Path, ci: Option<&CiEvent>, out: &mut dyn Write) -> Result<Verdict>`;
  subcommand `cargo xtask ui`.

- [ ] **Step 1: Write the failing tests**

In `ui.rs`: `the_ui_part_runs_fixed_pnpm_exec_calls`: `commands()` is exactly
`["install", "--frozen-lockfile"]`, `["exec", "tsc", "-b"]` (the template's tsconfigs set
`noEmit`, and they are protected), `["exec", "eslint", ".", "--max-warnings", "0"]`,
`["exec", "vitest", "run"]`, each run as `pnpm` in `web/`; none is `run <script>`.

In `tools.rs` (`// AC: TU-00.3`): `the_web_copies_equal_the_pins`: `web/.nvmrc` is the Node
pin, `web/package.json` `engines.node` is `^<node pin>` and `packageManager` is
`pnpm@<pnpm pin>`; each mismatch, and a missing file, is a finding naming the file. `tools`
checks these copies whenever it checks Node or pnpm.

`web/src/App.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import App from "./App";

describe("App", () => {
  // AC: TU-00.1
  it("renders the Loom heading", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "Loom" })).toBeDefined();
  });
});
```

- [ ] **Step 2: Run** `cargo nextest run -p xtask -E 'test(/ui/)'` — Expected: FAIL.

- [ ] **Step 3: Write `web/`**

`package.json`: `"private": true`, `"type": "module"`, `"packageManager": "pnpm@<pin>"`,
`"engines": { "node": "^<node pin>" }`, dev dependencies from the template plus `vitest`,
`jsdom`, `@testing-library/react`; scripts may exist for people but the gate never calls them.
`.nvmrc`: the Node pin. `tsconfig.app.json`: the template's with `"strict": true`.
`eslint.config.js`: the template's flat config (TypeScript, react-hooks, react-refresh).
`vite.config.ts`: the React plugin and `test: { environment: "jsdom" }`. `App.tsx` renders
`<h1>Loom</h1>` and nothing else; template assets, CSS and logos go.

- [ ] **Step 4: Implement `ui::run`** — find `pnpm` with `which::which("pnpm")` (on Windows the
`.cmd` shim), run each command in `web/` with an argument list; a failing command is a finding
naming it; later commands still run except after a failed `install`. A missing `pnpm` is an
error (the `tools` part reports versions).

- [ ] **Step 5: Run** the Step 2 command — Expected: PASS.

- [ ] **Step 6: Run the part on Windows** — `cargo xtask gate --only ui` — Expected: `pass  tools`,
`pass  ui`; the output shows tsc, ESLint and one passing Vitest test.

- [ ] **Step 7: Commit** — `feat(web): the web app skeleton and the ui part` (scope `web`; it
also adds `xtask/src/checks/ui.rs`, which the body says).

---

## Task 13: The docs-only rule and the `ui` list

**ACs:** TU-00.3

**Files:**
- Create: `gate/code-paths.toml`, `xtask/src/code_paths.rs`, `xtask/src/changes.rs`,
  `xtask/src/need.rs`
- Modify: `xtask/src/git.rs`, `xtask/src/gate.rs`, `xtask/src/lib.rs`,
  `xtask/src/checks/approval/mod.rs` (its changed paths from `changes`)

**Interfaces:**
- Produces:
  - `code_paths::FILE: &str = "gate/code-paths.toml"`; `pub struct CodePaths`;
    `CodePaths::load(root: &Path) -> Result<CodePaths>`; `CodePaths::parse(text: &str) -> Result<CodePaths>`;
    `fn is_docs_only(&self, paths: &[String]) -> bool`; `fn touches_ui(&self, paths: &[String]) -> bool`;
    `fn non_code(&self) -> &[String]`, `fn always_code(&self) -> &[String]`, `fn ui(&self) -> &[String]`.
  - `pub enum ChangedPaths { Known { branch: String, paths: Vec<String> }, NoRule { why: String }, Unreadable { why: String } }`;
    `pub fn changes::of_run(root: &Path, ci: Option<&CiEvent>) -> ChangedPaths`;
    `pub fn changes::of_pull_request(git: &Git, pr: &PullRequestEvent) -> Result<Vec<String>>`.
  - `Git::diff_name_only_worktree(&self, base: &str) -> Result<Vec<String>>` (`git diff
    --name-only --no-renames <base>`), `Git::untracked(&self) -> Result<Vec<String>>` (`git
    ls-files --others --exclude-standard`).
  - `pub enum Need { Run, NotNeeded(String) }`;
    `pub fn need::decide(check: Check, changes: &ChangedPaths, paths: &CodePaths) -> Need`.

- [ ] **Step 1: Write `gate/code-paths.toml`**

```toml
# Which changed paths can change which part's verdict (docs/gate.md §1.2, P00 spec §5.3).
# Globs against repo-relative paths with `/`; `*` stays in a folder, `**` crosses folders.
# non_code matches exactly; always_code ignores case and wins over non_code.
non_code = ["docs/**", "README.md", "CLAUDE.md", ".claude/**"]
always_code = [
    "docs/specs/00-umbrella.md", "docs/specs/phases/**", "docs/gate.md",
    "**/*.ts", "**/*.tsx", "**/Cargo.toml", "**/Cargo.lock", "**/rust-toolchain",
    "**/rust-toolchain.toml", "**/clippy.toml", "**/.clippy.toml", "**/rustfmt.toml",
    "**/.rustfmt.toml", "**/.cargo/**", "**/.gitignore", "**/.gitattributes",
    "web/package.json", "web/pnpm-lock.yaml",
]
# `ui` runs only when a changed path matches this list (and the run is not docs-only).
ui = ["web/**", "gate/tools.toml", "gate/code-paths.toml", "xtask/**", ".github/**"]
```

- [ ] **Step 2: Write the failing tests** (`// AC: TU-00.3`)

`need.rs`, over hand-built `ChangedPaths`:
- `a_docs_only_change_runs_only_commit_msg_and_secrets`: branch `docs/x`, paths
  `["docs/notes.md"]` → `CommitMsg` and `Secrets` `Run`; `Fmt`, `Test`, `Coverage`, `Ui`,
  `AcTrace` `NotNeeded` with a reason naming `gate/code-paths.toml`.
- `an_always_code_path_under_docs_runs_everything`: each of `docs/specs/00-umbrella.md`,
  `docs/specs/phases/P01-x.md`, `docs/gate.md`, `docs/x/Cargo.toml`, `docs/a.ts`, `docs/a.tsx`.
- `a_phase_branch_runs_everything`: branch `phase/P01-x`, paths `["docs/notes.md"]`.
- `no_rule_unreadable_or_empty_runs_everything`.
- `ui_runs_only_when_a_ui_path_changed`: `crates/loom-core/src/lib.rs` → `Ui` `NotNeeded`, `Test`
  `Run`; `web/src/App.tsx`, `gate/tools.toml`, `xtask/src/lib.rs` → `Ui` `Run`.
- `always_code_ignores_case_and_non_code_does_not`: `DOCS/notes.md` is code; `docs/x/CARGO.TOML`
  is always-code.

`code_paths.rs`: `the_real_lists_classify_every_input_a_part_reads`: code: `docs/specs/00-umbrella.md`,
`docs/gate.md`, `docs/specs/phases/P00-foundation-and-gate.md`, `CONTRIBUTING.md`,
`.github/workflows/ci.yml`, `.loom/protected.toml`, `.loom/checks.toml`, `gate/tools.toml`,
`web/src/App.tsx`, `notes/x.ts`, `Cargo.toml`, `crates/a/Cargo.toml`, `rust-toolchain.toml`,
`.cargo/config.toml`, `deny.toml`, `.gitleaksignore`; non-code: `docs/plans/P01-x.md`,
`docs/trial/P01-x.md`, `docs/decisions/0005-x.md`, `README.md`, `CLAUDE.md`,
`.claude/skills/x/SKILL.md`.

`changes.rs`, over a temp git repository with a bare `origin`:
- `a_pull_request_lists_the_paths_since_the_merge_base` (not those of `main` after the branch
  point).
- `a_rename_out_of_crates_is_a_deletion_and_an_addition`: `crates/a.rs` → `docs/a.rs` lists both.
- `locally_uncommitted_and_untracked_files_count`: on `docs/x`, a modified tracked `crates/a.rs`
  and an untracked `crates/b.rs` are both listed.
- `a_missing_base_commit_is_unreadable`; `a_push_or_a_detached_head_has_no_rule`.
- `changed_paths_use_slashes`: a file in a nested folder comes back as `a/b/c.rs` on every OS.

`gate.rs`: `a_docs_only_run_reports_every_other_part_as_not_needed` (over `run_checks` with the
decision; `tools` checks only the tools of the parts that run, here gitleaks);
`an_unreadable_change_list_runs_everything_with_a_note_on_tools`;
`a_broken_code_paths_file_stops_the_gate` (an `Error::Config` naming `gate/code-paths.toml`,
exit code 2: a broken gate is never a smaller run).

- [ ] **Step 3: Run** `cargo nextest run -p xtask -E 'test(/need|code_paths|changes|gate/)'` — Expected: FAIL.

- [ ] **Step 4: Implement** — P50 §5.3's five steps, in `need::decide` (never asked for
`Tools`, which always runs first); before them: `CommitMsg` and `Secrets` always `Run`; after
them: `Ui` is `NotNeeded` unless `touches_ui`. Globs with
`globset` and `literal_separator(true)`, `always_code` case-insensitive, as
`.loom/protected.toml`'s matcher does. `gate::run` decides every selected part first, gives
`tools::run_for` the parts that will run, runs those, and adds `Verdict::not_needed` for the
rest; an `Unreadable` note goes on the `tools` verdict. `approval` takes its paths from
`changes::of_pull_request`.

- [ ] **Step 5: Run** the Step 3 command and `cargo nextest run -p xtask --test approval` — Expected: PASS.

- [ ] **Step 6: Commit** — `feat(xtask): skip the parts a docs-only change cannot affect`.

---

## Task 14: `docs/gate.md`, `CONTRIBUTING.md`, the PR template and the repo contract

**ACs:** TU-00.2

**Files:**
- Modify: `docs/gate.md`, `CONTRIBUTING.md`, `.github/pull_request_template.md`,
  `xtask/README.md`, `gate/ruleset-main.json`, `gate/repo-settings.toml` (comments),
  `xtask/src/github/settings.rs` (`required_contexts_from_gate_md` keeps `ui`),
  `xtask/tests/settings.rs`, `xtask/tests/pr_body.rs`
- Create: `.loom/checks.toml`

**Interfaces:**
- Consumes: `code_paths::CodePaths` (Task 13), `approval::protected` (Task 7), `GATE_ORDER`.

- [ ] **Step 1: Write the failing tests** (`// AC: TU-00.2` unless said)

`settings.rs`:
- `ruleset_contexts_equal_gate_md_checks`: the contexts of `gate/ruleset-main.json` (each with
  `integration_id: 15368`, GitHub Actions) equal `docs/gate.md` §1's checks equal
  `{gate, test, coverage, ui, pr, approval}`.
- `checks_toml_lists_the_checks_of_gate_md`: `.loom/checks.toml` names the same six; every
  `command` starts `cargo xtask `; the parts (all checks together) are every `Check` but
  `Tools` and `Github`, each in exactly one check.
- `main_ruleset_encodes_r3`: rules `deletion`, `non_fast_forward`, `required_linear_history`,
  `pull_request` with `allowed_merge_methods: ["squash"]`, `required_status_checks` with
  `strict_required_status_checks_policy: true`; `bypass_actors` empty.
- `repo_settings_encode_section_6` as the editor's.
- `gate_md_lists_the_code_paths_exactly` (`// AC: TU-00.3`): §1.2's three lists equal
  `gate/code-paths.toml`.
- No new tests for §5 and the scopes: the editor's `protected_paths_toml_matches_gate_md_section_5`
  (`xtask/tests/approval.rs`, Task 7) and its `types_and_scopes_equal_contributing_md_section_2` (`xtask/tests/commit_msg.rs`,
  Task 3) keep them, retagged `// AC: TU-00.4`, and must stay green over the rewritten documents.
  (Plan change allowed by Jann during implementation, 27 Nov 2026.)

`pr_body.rs`: `the_template_has_every_section`: Phase, Acceptance criteria, Final review,
Rulings, Gate, Protected paths, Trial.

- [ ] **Step 2: Run** `cargo nextest run -p xtask --test settings --test pr_body` — Expected: FAIL.

- [ ] **Step 3: Write `.loom/checks.toml`**

```toml
# The checks of this repository (umbrella R2, docs/gate.md §1): each is a required status check
# on `main` and a CI job of the same name; `command` gives its verdict locally. P06 owns the
# full format.
[[check]]
name = "gate"
command = "cargo xtask gate --skip test,coverage,ui"
parts = ["fmt", "lints", "commit-msg", "secrets", "clippy", "docs", "deny", "ac-trace"]

[[check]]
name = "test"
command = "cargo xtask gate --only test"
parts = ["test"]

[[check]]
name = "coverage"
command = "cargo xtask gate --only coverage"
parts = ["coverage"]

[[check]]
name = "ui"
command = "cargo xtask gate --only ui"
parts = ["ui"]

[[check]]
name = "pr"
command = "cargo xtask pr --title <title> --body-file <file>"
parts = ["pr-title", "pr-body"]

[[check]]
name = "approval"
command = "cargo xtask approval"
parts = ["approval"]
```

- [ ] **Step 4: Write the documents**

`docs/gate.md`, the editor's structure with Loom's content:
§1 *Checks*: first table `Check | Runs | Parts` with the six rows above (D5: a check is a
required status check; parts are `cargo xtask` subcommands; `tools` runs first in every
`cargo xtask gate` job; `approval` runs only on GitHub); §1.1 *Parts*: what each part runs (the
editor's §1 wording, plus `secrets` and `ui`); §1.2 *Docs-only rule*: the three lists of
`gate/code-paths.toml` as bullet lists in that order, P50 §5.3's five steps plus "commit-msg and
secrets always run" and the `ui` list, and a table of every part with why a non-code change
can't change its verdict (P50 §5.4, Loom's parts; `ac-trace` reads the phase specs, which are
always-code); §1.3 *Speed*: the budgets of spec §5.7. §2 Lints, §3 Thresholds (Loom's classes),
§4 AC tags (TU ids; scope from the phase spec's §2; a phase built in another repo demands
nothing), §5 Protected paths (the list of `.loom/protected.toml`; labels `needs-jann` and
`approved`), §6 Merging (squash only, linear history, up-to-date branches, no direct, force or
delete pushes; R3's `loom-*` checks come with P07; `cargo xtask github check`/`apply`), §7
Process requirements outside CI. No release, Windows-CI or macOS text.

`CONTRIBUTING.md`: §1 Branches (`phase/PNN-<slug>`, `spec/PNN-<slug>`, `gate/<slug>`,
`docs/<slug>`, `fix/<slug>`); §2 Commits (types as the editor's; the 13 scopes; footers
`Refs:` with TU ids, `Phase:`, the Claude trailers); §3 Pull requests (title rules; update a
branch from `main` only when it is ready to merge and next in line; push when a task or review
round is done — P50 §5.8); §4 Tests (`// AC:` tags, `slow_` names kept for a later split,
`LOOM_TEST_DATABASE_URL` for tests that need PostgreSQL from P02); §5 Documentation; §6
Language and files. No versions or tags section.

`.github/pull_request_template.md`: the editor's sections, examples with `TU-` ids, and a last
section `## Trial` (`<!-- Link to docs/trial/PNN-<slug>.md and the numbers of this step. -->`);
`## Gate` asks for the summary of `cargo xtask gate` on the final commit (Windows or Linux).

`gate/ruleset-main.json`: the editor's, with `required_status_checks` = `gate`, `test`,
`coverage`, `ui`, `pr`, `approval`, each `integration_id: 15368`.
`xtask/README.md`: Loom's subcommands and modules.

- [ ] **Step 5: Run** the Step 2 command, `cargo nextest run -p xtask --test approval --test commit_msg`
and `cargo nextest run -p xtask -E 'test(/settings|conventional|pr_body/)'` — Expected: PASS.

- [ ] **Step 6: Commit** — `docs(gate): Loom's gate, contributing rules and PR template`.

---

## Task 15: The workflows

**ACs:** TU-00.1, TU-00.2, TU-00.3

**Files:**
- Modify: `.github/workflows/ci.yml`, `.github/workflows/pr.yml`, `.github/workflows/approval.yml`,
  `.github/workflows/settings.yml`, `.github/actions/setup/action.yml`, `xtask/tests/workflows.rs`
- Create: `.github/actions/xtask/action.yml` (the cached xtask; P50 §10 leaves its shape to the
  build within §5.5)

**Interfaces:**
- Consumes: `gate::selection`/`gate::selected` (Task 9) from the tests; `cargo xtask tools
  --print-install-spec --for <parts>` (Task 10).

- [ ] **Step 1: Write the failing tests** (`xtask/tests/workflows.rs`, rewritten from the editor's)

- `ci_runs_four_jobs_side_by_side_on_linux` (`// AC: TU-00.1, TU-00.3`): `ci.yml` jobs are
  exactly `gate`, `test`, `coverage`, `ui`; none has `needs:` or a job-level `if:`; all
  `runs-on: ubuntu-latest`; triggers `pull_request` (opened, synchronize, reopened), `push` to
  `main`, `workflow_dispatch`.
- `each_ci_job_runs_its_check_command` (`// AC: TU-00.3`): each job's last step runs the
  `command` of its check in `.loom/checks.toml`: either that command as written, or the
  cached-xtask action with the command's arguments after `cargo xtask `.
- `the_ci_jobs_cover_every_gate_part_once` (`// AC: TU-00.3`): parsing each job's
  `--only`/`--skip` with `gate::selection` and `gate::selected`, the parts other than `tools` of
  the four jobs are disjoint and together are `GATE_ORDER` without `Tools`.
- `required_contexts_are_the_jobs` (`// AC: TU-00.2`): the ruleset's contexts equal the jobs of
  `ci.yml`, `pr.yml` and `approval.yml`; `settings` is not one.
- `pr_runs_both_parts_in_one_job` (`// AC: TU-00.3`): `pr.yml` has one job `pr` running
  `cargo xtask pr` on `opened, edited, synchronize, reopened`.
- `build_caches_are_saved_only_on_main`: rust-cache's `save-if` is `github.ref ==
  'refs/heads/main'` in `gate`, `test` and `coverage`; `ui`, `pr` and `approval` use no
  rust-cache.
- `the_xtask_cache_is_saved_by_gate_on_main_and_restored_by_exact_key`: `gate` saves
  `target/xtask-bin/debug/xtask` only on `main`, under a key that hashes `xtask/**`,
  `Cargo.lock`, `rust-toolchain.toml` and `.cargo/**` with the runner OS; `ui`, `pr` and
  `approval` restore with the same key and no `restore-keys`, and never save it.
- `test_and_coverage_get_a_throwaway_postgres`: a `postgres:18` service with a health check and
  `POSTGRES_HOST_AUTH_METHOD: trust`; job env `LOOM_TEST_DATABASE_URL:
  postgres://postgres@localhost:5432/loom_test`; no other job has a service.
- `only_the_ui_job_sets_up_node`: `actions/setup-node` with `node-version-file: web/.nvmrc`,
  `cache: pnpm`, `cache-dependency-path: web/pnpm-lock.yaml`, after pnpm is set up from
  `packageManager`.
- `history_is_fetched_where_changes_or_commits_are_read`: `fetch-depth: 0` in `gate`, `test`,
  `coverage`, `ui` and `approval`.
- Kept from the editor, adapted to these jobs: `no_key_lets_a_job_pass_while_xtask_fails`,
  `no_key_lets_a_step_of_the_setup_action_fail_silently`, `every_job_has_a_timeout`,
  `required_jobs_always_report`, `no_pull_request_target_and_least_privilege`,
  `token_reaches_only_the_steps_that_call_github`,
  `only_the_approval_checkouts_keep_the_git_credentials`, `actions_pinned_by_sha`,
  `no_expressions_in_run_steps`, `coverage_restores_and_saves_the_baseline_cache`,
  `triggers_and_concurrency_follow_the_spec` (`ci` and `pr` cancel older pull-request runs;
  `approval` never cancels).

- [ ] **Step 2: Run** `cargo nextest run -p xtask --test workflows` — Expected: FAIL.

- [ ] **Step 3: Write the workflows** — P50 §5.2 and §5.5 with Loom's four `ci` jobs; timeouts
30 minutes until Task 16 measures them; every action pinned by SHA with its version in a
comment, current releases (spec §9). The setup action: install the toolchain, restore
rust-cache (`shared-key` = job, `save-if` from input), install the tools of
`cargo xtask tools --print-install-spec --for <the job's parts>` with `taiki-e/install-action`,
and gitleaks per the install ruling in the `gate` job. The cached-xtask action: restore by
exact key; on a miss install the toolchain and `cargo build --locked -p xtask --target-dir
target/xtask-bin`; then run `target/xtask-bin/debug/xtask --root . <args>`, with the
arguments passed through `env:`, never as `${{ }}` in `run:`. `approval.yml` keeps the
editor's two checkouts (base, and head while the base has no `xtask/Cargo.toml`).

- [ ] **Step 4: Run** `cargo nextest run -p xtask --test workflows` — Expected: PASS. Then the
whole suite: `cargo nextest run --workspace --locked` — Expected: PASS, none red from here on.

- [ ] **Step 5: Commit** — `ci: the gate in four side-by-side jobs, pr and approval`.

---

## Task 16: Full gate, pull request and CI proof

**ACs:** TU-00.1, TU-00.2, TU-00.3, TU-00.4 (proof on the PR)

**Files:**
- Modify: `.github/workflows/*.yml` (timeouts), `docs/trial/P00-foundation-and-gate.md`

- [ ] **Step 1: Run the full local gate on Windows** — `cargo xtask gate` — Expected: every part
`pass` (on `phase/P00-…` none is *not needed*), `ac-trace` lists TU-00.1 to TU-00.4 each with a
test. Read the whole output; save the final table for the PR.

- [ ] **Step 2: Bring the branch up to date if `main` moved** — `git fetch origin && git merge
origin/main` (a merge commit; never a rebase of pushed commits). Re-run Step 1 if anything came in.

- [ ] **Step 3: Push and open the PR** — title `feat(gate): the foundation and the gate (P00)`;
the template filled: every AC with its tests, every `Ruling:` of this plan and of the build, the
local gate table (Windows), every protected path touched, the trial record link. Under
*Protected paths* ask Jann to run `cargo xtask github apply` after his review, so the ruleset
and labels exist (TU-00.2 live). Don't enable auto-merge; don't run `apply`.

- [ ] **Step 4: Read the cold CI run** — every job green; `gh run view <id> --json jobs` gives
each job's duration. Set each `timeout-minutes` to three times its cold duration, rounded up to
5, at least 10; commit `ci: timeouts from the cold run`; push.

- [ ] **Step 5: Compare CI with the local gate on the same commit** (TU-00.3) — each part's
verdict in the four job summaries and the `pr` summary equals the local table's; write the
comparison into the PR.

- [ ] **Step 6: Probe the docs-only rule and the `ui` list in CI** — two branches off the phase
branch, each a PR into it, closed unmerged afterwards and linked from the P00 PR:
`docs/probe-docs-only` adds `docs/trial/probe.md` → `gate` runs `commit-msg` and `secrets`,
every other part (and all of `test`, `coverage`, `ui`) reports *not needed*;
`fix/probe-ui-list` changes a doc comment in `crates/loom-core/src/lib.rs` → `ui` reports *not
needed*, `test` runs. Delete both branches after closing.

- [ ] **Step 7: Record the trial numbers** — Step 2 of the trial record: tokens, cost, wall time,
plan gaps raised and plan changes allowed; and the cold-run times of each job.

---

## Self-review notes

- Spec coverage: §5.1 → Tasks 1, 2, 12; §5.2 → Tasks 9–15 (linker: ruling); §5.3 → Tasks 6, 7,
  8, 10, 13, 14; §5.4 → Tasks 3–5, 7, 8, 11, 12, 14; §5.5 → Task 12; §5.6 → Task 2; §5.7 →
  Tasks 15, 16; §5.8 → Task 14; §6 → the tagged tests of Tasks 2–15 and Task 16; §7
  (`LOOM_TEST_DATABASE_URL`, P04 rule, `--only`/`--skip`, Trial section) → Tasks 15, 3, 9, 14.
- TU-00.2's live half (`cargo xtask github check` against GitHub) passes only after Jann's
  `apply` (spec §8); the PR says so and the review step checks it.
