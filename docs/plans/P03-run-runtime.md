# P03 · Run runtime — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to
> implement this plan task-by-task, in a fresh session (`CLAUDE.md`). Steps use checkbox
> (`- [ ]`) syntax for tracking.

**Goal:** `loom-run` runs one Claude Code CLI session for a story in a rootless Podman container
with its own clone, the pinned kit and a filtered network. It turns the stream into typed events,
stores the raw stream in PostgreSQL, and stops a run on cancel, a cap, a rate limit or an
authentication failure, recording the cause.

**Architecture:** `loom-core::run` holds the domain types: events, stop causes, usage and the
store traits. `loom-store` implements the traits on PostgreSQL (the store's first migration).
`loom-run` is split one job per module (config, workspace, kit, stream, usage, prices, caps,
launcher, runner, podman, egress). The runner talks to a `Launcher` seam with two implementations:
a local process (every platform; fast tests) and Podman with a Squid proxy (Linux only; the
container ACs).

**Tech Stack:** Rust 1.99.1 (edition 2024), `tokio` (process, time, sync, io), `tokio-util`
(`CancellationToken`), `serde`/`serde_json`/`toml`, `uuid`, `sqlx` (postgres, runtime queries,
`migrate!`), `thiserror`, `tracing`, an HTTP client (`ureq` or `reqwest`, see Task 4), `git` and
`podman` CLIs, Squid, PostgreSQL 18.

**Spec:** `docs/specs/phases/P03-run-runtime.md` (approved by Jann, merged in #12, 28 Nov 2026).
Read it first; this plan cites its decisions (D1–D9) and rulings (R-a–R-p). Decisions 0010–0018
are in `docs/decisions/`. Umbrella version 9.

**Order and merges (decision 0010).** P03 runs before P02, while P01 is built in parallel. Both add
workspace dependencies: **before opening the PR, merge `origin/main` into the branch** (a merge
commit, never a rebase or force push), resolve `Cargo.toml` and `Cargo.lock` (regenerate the lock
with `cargo update --workspace` if needed, never hand-edit it), and rerun the full gate. If
decision numbers clash with ones that landed meanwhile, the PR that merges second renumbers its
own.

---

## Files and areas P03 touches

No protected path (`docs/gate.md` §5) is changed by this plan. The PR can merge on green.

| Area | Paths | Tasks |
|---|---|---|
| Workspace dependencies | `Cargo.toml` (`[workspace.dependencies]` only, never `[workspace.lints]`), `Cargo.lock` | 1, 2, 4, 7 |
| Domain types | `crates/loom-core/{Cargo.toml,README.md,src/lib.rs,src/run/**}` | 1 |
| Store | `crates/loom-store/{Cargo.toml,README.md,src/**,migrations/0001_run_runtime.sql,tests/**}` | 2 |
| Runtime | `crates/loom-run/{Cargo.toml,README.md,src/**}` | 3–10 |
| Fake CLI | `crates/loom-run/examples/fake_claude.rs` | 7 |
| Tests and fixtures | `crates/loom-run/tests/**`, `crates/loom-run/tests/fixtures/**`, `crates/loom-store/tests/**` | 2–10 |
| Trial record | `docs/trial/P03-run-runtime.md` | 10 |

Implementers touching anything else stop and ask (implement step 3 of `loom-phase`).

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
  worktree and branch, merged back one at a time. Network only to public HTTP(S)
  destinations, through a proxy that records every destination in the run's log, so research
  tools work (decision 0013). No access to the home LAN, the database VM or any other private
  address range. Credentials: a
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
- **T14** New models and new versions of the Claude Code CLI keep working without a code change
  or a new release of Loom. Model names and prices are data, loaded while Loom runs (a public
  price list, with Loom's config overriding it). Every run uses the latest CLI and records the
  version it used (decision 0016).

## Acceptance criteria (umbrella §9 P03, verbatim)

**TU-03 Running Claude Code in a container**
- TU-03.1 `loom-run` starts a rootless container from the configured image, with a fresh git
  worktree of the story branch and the workflow kit at the pinned version. No two runs ever share
  a worktree (tested with two concurrent runs of one story).
- TU-03.2 It runs `claude -p` with streamed JSON output, parses every event into typed run
  events, and stores the raw stream (T7).
- TU-03.3 A run can be cancelled. Cancelling stops the whole process tree and the container
  within 10 s.
- TU-03.4 Token and time caps per run stop it. The cause is recorded.
- TU-03.5 The container's network allows only the destinations of T4 (tested with a blocked
  request).
- TU-03.6 Rate-limit and authentication errors from the CLI become distinct typed events.

How the criteria are read (spec §2, decided by Jann): "fresh git worktree" is a per-run local
clone (D3); "the destinations of T4" are public HTTP(S) through the proxy, with private ranges
blocked (D4); the token caps are one per token kind, plus a spend cap (D6).

## Brief for every subagent

Put this in every implementer, reviewer and fix brief:

- `CLAUDE.md` binds you: its hard rules (never weaken a check, no `#[allow]`, no
  `#[ignore]`/`#[expect]` without a reason, never `--no-verify`, never claim a check passed
  without running it and reading its output) and its six engineering directives. Directive 2:
  use a standard tool or a well-known crate before custom code.
- Read the spec section your task names. Every deviation from the spec or this plan is a
  `Ruling: <what> — <why> — <cost if wrong>` line you report back.
- Library errors with `thiserror`, no `unwrap`/`expect`/`panic` outside tests, `tracing` for
  logs, every public item documented (`missing_docs` is denied).
- Container code is `#[cfg(target_os = "linux")]` (D2); everything else builds and is tested on
  Windows and Linux. A test file that needs Podman starts with `#![cfg(target_os = "linux")]`.
- Tests that need PostgreSQL read `LOOM_TEST_DATABASE_URL` through `loom_store::testing`
  (Task 2) and never skip: without the variable they fail with the message Task 2 defines.
  Locally: `docker run -d --rm --name loom-pg -p 5432:5432 -e POSTGRES_HOST_AUTH_METHOD=trust postgres:18`
  then `LOOM_TEST_DATABASE_URL=postgres://postgres@localhost:5432/postgres`.
- Tests that prove a criterion carry `// AC: TU-03.N` directly above `#[test]`/`#[tokio::test]`.
- Secrets: a credential's value never appears in an argument, an event, a log line or a fixture.
- Commits: Conventional Commits, scope `run`, `store` or `core`, footer `Phase: P03`, trailers
  `Co-Authored-By: <your model> <noreply@anthropic.com>` and `Claude-Session: <session>`.
- Run per task: `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked -- -D warnings`
  and the task's tests. The full gate runs in Task 10.

## Rulings (Claude's calls; listed in the PR)

The spec's R-a to R-p hold. The plan adds:

- **Ruling: one fake CLI, a Rust example (`examples/fake_claude.rs`), for local and container
  tests; the container test image is a pinned `debian:trixie-slim` instead of Alpine (refines
  R-m)** — one fake to maintain, it runs on Windows for the local tests, and a glibc image runs
  the binary built on the Ubuntu runner without cross-compiling — if wrong, the container tests
  need a static (musl) build or a second fake.
- **Ruling: the container lives as long as the run: `podman run -d --init … sleep infinity`,
  then `podman exec` for `claude update`, `claude --version` and `claude -p`** — the update and
  version output stay out of the JSON stream, and `podman stop` ends every exec at once — if
  wrong, one `sh -c` command line instead, with a marker line separating the prelude from the
  stream.
- **Ruling: credentials pass as `--env NAME` with no value**, so Podman copies them from Loom's
  environment and no value is ever on a command line — if wrong, values would show in `ps`.
- **Ruling: Squid joins the run's internal network and one egress network**, named in the config
  (`egress_network`, default `loom-egress`, created when missing); the tests use a network with a
  `198.18.N.0/24` subnet as the egress network, so "public" needs no internet — if wrong, the
  allowed-request test needs the internet.
- **Ruling: Squid allows HTTP and `CONNECT` to any port; the address filter is the guard** —
  T4 v8 says public HTTP(S) destinations, and registries and tools use more than 80 and 443 — if
  wrong, a port list in the generated config.
- **Ruling: `git clone --local` keeps hard links (D3)**; git's object files are read-only, so
  changing the mirror through them takes a deliberate `chmod` inside a run, which is left to the
  reviews (directive 4) — if wrong, `--no-hardlinks` costs disk and time per run.
- **Ruling: one in-process lock per mirror** (`tokio::sync::Mutex` in a map keyed by mirror path)
  serialises fetches; one Loom service owns the data dir — if wrong, two Loom processes on one
  data dir could fetch at once (git's own lock files then refuse one).
- **Ruling: money is `f64` US dollars**; caps are compared, never summed into accounts — if
  wrong, P17 needs a decimal type for spend reports.
- **Ruling: a model's price is looked up by the exact name, then `anthropic/<name>`**; a kind the
  run used whose price is missing counts as no price (R-g), including the 1-hour cache-write price
  — if wrong, runs with a spend cap stop until an override is added.
- **Ruling: the local fake is found next to the test binary** (`target/<profile>/examples/`),
  relying on Cargo building examples with tests — if wrong, a `build.rs`-free alternative is an
  `[[example]]` built explicitly by the test support with `cargo build --example`.
- **Ruling: test subnets `198.18.N.0/24` pick a free `N`** and retry on a clash, so container
  tests run in parallel under nextest without a test-group change in the protected nextest config.

## Review Focus

1. **A run whose story branch doesn't exist yet**: the clone creates it from the default branch,
   and the push remote is GitHub's URL, not the mirror's. Test: Task 6
   `a_missing_story_branch_starts_from_the_default_branch`.
2. **A secret in the environment**: the token's value never shows in the podman arguments, the
   events or the raw stream Loom adds itself. Test: Task 8
   `credentials_pass_by_name_never_by_value`.
3. **The CLI prints a line that is not JSON, or a JSON line of an unknown type** (a warning, a new
   event of a newer CLI): an `Unknown` event, the run goes on, the raw store has it byte for byte.
   Test: Task 3 `a_line_that_is_not_json_is_kept_as_unknown`.
4. **A run that goes silent** (the CLI hangs without output): the time cap still stops it, from
   the timer. Test: Task 7 `a_silent_run_stops_at_its_time_cap`.
5. **A test fails half way**: its containers, networks and databases are removed anyway, so the
   next test and the next CI run start clean. Test: Task 8
   `a_failed_start_leaves_nothing_behind`.

---

## Task 1: Workspace dependencies and the run domain types

Spec §5.1 (`loom-core::run`), §5.4, §5.5, R-d, R-e, R-o.

**Files:**
- Modify: `Cargo.toml` (`[workspace.dependencies]`: add `tokio`, `tokio-util`, `uuid`,
  `serde`/`serde_json` already present), `crates/loom-core/Cargo.toml`, `crates/loom-core/src/lib.rs`,
  `crates/loom-core/README.md`
- Create: `crates/loom-core/src/run/mod.rs`, `run/event.rs`, `run/usage.rs`, `run/stop.rs`,
  `run/store.rs`

**Interfaces:**
- Produces (all in `loom_core::run`, `Serialize`/`Deserialize`, `Debug`, `Clone`, `PartialEq`):
  - `RunId(Uuid)` with `RunId::new()` (v4) and `Display`.
  - `StreamKind { Stdout, Stderr }`.
  - `TokenKind { Input, Output, CacheWrite, CacheRead }`.
  - `Usage { input: u64, output: u64, cache_write_5m: u64, cache_write_1h: u64, cache_read: u64 }`
    with `fn get(&self, kind: TokenKind) -> u64` (CacheWrite = 5m + 1h) and `Add`.
  - `CapKind { Tokens(TokenKind), Spend, WallTime }`.
  - `StopCause { Completed, Failed { exit_code: Option<i32> }, Cancelled, Cap(CapKind),
    NoPrice { model: String }, RateLimited { resets_at: Option<i64> }, AuthFailed }`.
  - `RunEvent` (internally tagged `"event"`): `Image { id }`, `Kit { repository, version }`,
    `CliVersion { version }`, `CliUpdateFailed { message }`, `Init { session_id, model,
    cli_version }`, `Assistant { message_id, model, usage: Option<Usage>, raw: Value }`,
    `User { raw: Value }`, `System { subtype, raw: Value }`, `RateLimitStatus { kind, resets_at }`,
    `RateLimited { kind: Option<String>, resets_at: Option<i64> }`, `AuthFailed { message }`,
    `Result { subtype, is_error, total_cost_usd: Option<f64>, raw: Value }`,
    `Egress { host, port: Option<u16>, allowed: bool }`, `SpendMismatch { computed_usd,
    reported_usd }`, `Unknown { raw: String }`, `RunEnded { cause: StopCause }`.
    `Unknown.raw` is the line as text (lossy UTF-8), so a non-JSON line is kept too.
  - `trait RawStreamStore: Send + Sync` with `type Error: std::error::Error + Send + Sync + 'static`,
    `fn append(&self, run: RunId, stream: StreamKind, chunk: &[u8]) -> impl Future<Output = Result<(), Self::Error>> + Send`
    and `fn read(&self, run: RunId, stream: StreamKind) -> impl Future<Output = Result<Vec<u8>, Self::Error>> + Send`.
  - `trait PriceListStore: Send + Sync` with the same `Error`, `save(&self, source_url: &str, body: &str)`
    and `latest(&self) -> Result<Option<String>, _>` (both async as above).

- [ ] **Step 1: Write the failing tests** in `run/usage.rs` and `run/event.rs`:
  `usage_get_sums_both_cache_write_parts` (5m 3 + 1h 4 → `get(CacheWrite) == 7`),
  `usage_adds_field_by_field`, `every_event_round_trips_through_json` (one value of each variant,
  `serde_json::to_string` then `from_str` equals), `a_stop_cause_names_its_cap`
  (`Cap(Tokens(CacheRead))` serialises to JSON containing `"CacheRead"`).
- [ ] **Step 2: Run** `cargo nextest run -p loom-core` — FAIL (types missing).
- [ ] **Step 3: Implement** the types above; add `tokio = { version = "1", features = ["rt-multi-thread", "macros", "process", "io-util", "sync", "time", "fs"] }`,
  `tokio-util = "0.7"`, `uuid = { version = "1", features = ["v4", "serde"] }` to
  `[workspace.dependencies]` (latest compatible versions at build time). Update the crate README
  and the `lib.rs` doc comment (it is no longer empty).
- [ ] **Step 4: Run** `cargo nextest run -p loom-core` and `cargo clippy -p loom-core --all-targets --locked -- -D warnings` — PASS, no warnings.
- [ ] **Step 5: Commit** `feat(core): run events, stop causes and the store traits`.

## Task 2: The raw stream and the price list in PostgreSQL

Spec §5.6, R-h, R-i, D8. AC: TU-03.2 (stores the raw stream).

**Files:**
- Modify: `Cargo.toml` (add `sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "tls-rustls-ring", "postgres", "uuid", "migrate", "macros"] }`
  — `macros` only for `migrate!`; use the newest 0.8.x, and if its TLS feature names differ, the
  one on `ring`, never `aws-lc-rs`), `crates/loom-store/Cargo.toml`, `src/lib.rs`, `README.md`
- Create: `crates/loom-store/migrations/0001_run_runtime.sql`, `src/raw_stream.rs`,
  `src/price_list.rs`, `src/migrate.rs`, `src/testing.rs`, `tests/raw_stream.rs`,
  `tests/price_list.rs`

**Interfaces:**
- Consumes: `RawStreamStore`, `PriceListStore`, `RunId`, `StreamKind` (Task 1).
- Produces:
  - `loom_store::migrate(pool: &PgPool) -> Result<(), StoreError>` (runs `sqlx::migrate!()`).
  - `PgRawStreamStore::new(pool: PgPool)`, implementing `RawStreamStore`; `seq` is assigned in
    the insert (`coalesce(max(seq), -1) + 1` per run, inside one statement).
  - `prune_raw_streams(pool: &PgPool, older_than: std::time::Duration) -> Result<u64, StoreError>`
    (rows deleted; `written_at < now() - make_interval(secs => $1)`).
  - `PgPriceListStore::new(pool: PgPool)`, implementing `PriceListStore` (`latest` = newest
    `fetched_at`).
  - `StoreError` (`thiserror`).
  - `loom_store::testing::TestDb` (public and documented, for the tests of `loom-store` and `loom-run`):
    `TestDb::create().await -> Result<TestDb, StoreError>` reads `LOOM_TEST_DATABASE_URL`, creates
    a database `loom_test_<uuid>`, migrates it, `TestDb::pool()`, and drops the database in
    `TestDb::drop_db(self).await`. Without the variable: an error whose message is
    `LOOM_TEST_DATABASE_URL is not set; start a throwaway PostgreSQL (see crates/loom-store/README.md)`.

Migration (spec §5.6; the column list is fixed):

```sql
create table run_stream_chunks (
    run_id uuid not null,
    seq integer not null,
    stream text not null check (stream in ('stdout', 'stderr')),
    written_at timestamptz not null default now(),
    body bytea not null,
    primary key (run_id, seq)
);
create index run_stream_chunks_written_at on run_stream_chunks (written_at);
create table price_lists (
    fetched_at timestamptz primary key default now(),
    source_url text not null,
    body jsonb not null
);
```

- [ ] **Step 1: Write the failing tests** in `tests/raw_stream.rs`:
  `// AC: TU-03.2` `appended_chunks_read_back_byte_for_byte` (stdout chunks `b"{\"a\":1}\n"` and
  `b"\xff\x00raw\n"`, a stderr chunk; `read` per stream returns the concatenation exactly, and the
  other stream's bytes never mix in); `chunks_of_two_runs_stay_apart`;
  `prune_deletes_only_older_chunks` (set one chunk's `written_at` to 31 days ago with a direct
  `update`; `prune_raw_streams(30 days)` returns 1, the fresh chunk stays).
  In `tests/price_list.rs`: `latest_returns_the_newest_copy`, `latest_is_none_when_empty`.
  Plus `testing_without_the_variable_says_how_to_start_one` (unit test in `testing.rs`, with the
  variable read through a function that takes the value, so the test doesn't touch the process
  environment).
- [ ] **Step 2: Run** `cargo nextest run -p loom-store` with `LOOM_TEST_DATABASE_URL` set — FAIL.
- [ ] **Step 3: Implement** with runtime `sqlx::query` (R-i). README: what the crate stores, the
  migration rule (P02 adds `0002_…` after this one) and the local throwaway-database command from
  the brief.
- [ ] **Step 4: Run** the tests — PASS; then clippy and `cargo deny --locked check` — PASS.
- [ ] **Step 5: Commit** `feat(store): raw run streams and price lists in PostgreSQL`.

## Task 3: Parsing the CLI's stream

Spec §5.4, R-e. AC: TU-03.2, TU-03.6.

**Files:**
- Modify: `crates/loom-run/Cargo.toml` (deps: `loom-core`, `serde`, `serde_json`, `thiserror`,
  `tracing`)
- Create: `crates/loom-run/src/stream.rs`, `tests/fixtures/streams/README.md`,
  `tests/fixtures/streams/{normal_run.jsonl, auth_failure.jsonl, rate_limit_rejected.jsonl,
  result_429.jsonl}`

**Interfaces:**
- Produces: `loom_run::stream::parse_line(line: &[u8]) -> RunEvent`. Never fails: anything that
  isn't a known JSON event becomes `Unknown { raw }`.

Mapping (spec §5.4): `system`/`init` → `Init`; other `system` → `System`; `assistant` → `Assistant`
(`message.id`, `message.model`, `message.usage` → `Usage`: `input_tokens`, `output_tokens`,
`cache_read_input_tokens`, and `cache_creation.ephemeral_5m_input_tokens` /
`ephemeral_1h_input_tokens` when present, else all of `cache_creation_input_tokens` as 5m);
`user` → `User`; `rate_limit_event` with `rate_limit_info.status == "allowed"` → `RateLimitStatus`,
any other status → `RateLimited`; `result` with `api_error_status == 429` → `RateLimited`, with
401 or 403, or an `is_error` result whose text says the CLI isn't signed in
(`"Invalid API key"`, `"/login"`, `"authentication"`, case-insensitive) → `AuthFailed`, else
`Result`.

Fixtures: `normal_run.jsonl` and `auth_failure.jsonl` are **real** recordings
(`claude -p "Reply with hi." --output-format stream-json --verbose --model haiku --max-turns 1`,
the second with `CLAUDE_CODE_OAUTH_TOKEN=invalid`), with `session_id`, `uuid`, account and
organisation ids replaced by fixed fake values and the hook events of the recording machine
removed. The rate-limit fixtures are **synthetic**. The fixtures' README says which is which,
when and with which CLI version they were recorded (JSONL has no comments).

- [ ] **Step 1: Write the failing tests** (unit tests in `stream.rs`):
  `// AC: TU-03.2` `every_line_of_a_real_run_becomes_a_typed_event` (each line of `normal_run.jsonl`
  via `include_bytes!`; no `Unknown`; the `Assistant` usage equals the recording's numbers);
  `// AC: TU-03.2` `a_line_that_is_not_json_is_kept_as_unknown` (`b"Warning: x"` →
  `Unknown { raw: "Warning: x" }`); `an_unknown_event_type_is_kept_as_unknown`
  (`{"type":"brand_new"}`); `// AC: TU-03.6` `a_rejected_rate_limit_is_rate_limited`;
  `// AC: TU-03.6` `an_allowed_rate_limit_is_only_status`; `// AC: TU-03.6`
  `a_429_result_is_rate_limited`; `// AC: TU-03.6` `an_auth_failure_is_auth_failed_not_rate_limited`
  (from `auth_failure.jsonl`); `cache_writes_split_into_5m_and_1h`.
- [ ] **Step 2: Run** `cargo nextest run -p loom-run stream` — FAIL.
- [ ] **Step 3: Record the real fixtures, scrub them, implement `parse_line`** with `serde_json`
  (`Value` access; no hand parsing).
- [ ] **Step 4: Run** — PASS. Check the fixtures hold no real id: `gitleaks` runs in the gate;
  also grep them for the recording machine's user name and account email — none.
- [ ] **Step 5: Commit** `feat(run): typed events from the CLI's stream`.

## Task 4: Usage and prices

Spec §5.5, D6, D7, R-g. AC: TU-03.4.

**Files:**
- Modify: `Cargo.toml` and `crates/loom-run/Cargo.toml` (the HTTP client: `ureq` 3 with its
  `rustls` feature, or `reqwest` with `rustls-tls` on `ring` — whichever passes `cargo deny`
  unchanged; check before choosing), `src/lib.rs`
- Create: `crates/loom-run/src/usage.rs`, `src/prices.rs`,
  `tests/fixtures/prices/litellm_subset.json` (synthetic: a top-level key `"_comment"` says so),
  `tests/prices.rs`

**Interfaces:**
- Consumes: `Usage`, `TokenKind`, `PriceListStore` (Task 1), `RunEvent::Assistant` (Task 3).
- Produces:
  - `UsageLedger::default()`, `record(&mut self, message_id: &str, model: &str, usage: Usage)`
    (the last value of a message id wins), `total(&self) -> Usage`,
    `by_model(&self) -> BTreeMap<String, Usage>`.
  - `ModelPrice { input, output, cache_write_5m, cache_write_1h, cache_read }`, each
    `Option<f64>` USD per token.
  - `PriceBook::from_litellm(json: &str, overrides: &BTreeMap<String, ModelPrice>) -> Result<PriceBook, PriceError>`
    (LiteLLM keys: `input_cost_per_token`, `output_cost_per_token`,
    `cache_creation_input_token_cost`, `cache_creation_input_token_cost_above_1hr`,
    `cache_read_input_token_cost`; entries that aren't objects, like `_comment` or
    `sample_spec`, are skipped; an override field replaces the list's field).
  - `PriceBook::spend(&self, by_model: &BTreeMap<String, Usage>) -> Result<f64, NoPrice>` where
    `NoPrice { model: String }`: a kind with tokens > 0 and no price is `NoPrice`.
  - `prices::refresh(url: &str, store: &impl PriceListStore, overrides) -> Result<PriceBook, PriceError>`:
    fetch; if it parses, save it and use it; else load `store.latest()`, log a `tracing::warn!`,
    and use that; no copy at all → `PriceError::NoPriceList`.

- [ ] **Step 1: Write the failing tests**: in `usage.rs` `a_repeated_message_counts_once`
  (record id `m1` twice with different usage → total equals the second);
  `usage_sums_across_messages_and_models`. In `tests/prices.rs`: `// AC: TU-03.4`
  `spend_prices_each_kind_at_its_own_price` (fixture prices × fixed usage = exact expected
  dollars, compared with a tolerance of 1e-12); `an_override_beats_the_list`;
  `// AC: TU-03.4` `a_model_without_a_price_is_no_price`; `a_missing_1h_price_is_no_price_only_when_used`;
  `the_anthropic_prefix_is_tried_second`; `refresh_saves_a_good_list` and
  `refresh_falls_back_to_the_last_good_copy` (a `std::net::TcpListener` on `127.0.0.1:0` serves
  the fixture, then garbage; the store is a `PgPriceListStore` on a `TestDb`).
- [ ] **Step 2: Run** `cargo nextest run -p loom-run usage prices` — FAIL.
- [ ] **Step 3: Implement**; the fetch runs in `tokio::task::spawn_blocking` if the client is
  blocking.
- [ ] **Step 4: Run** — PASS; `cargo deny --locked check` — PASS.
- [ ] **Step 5: Commit** `feat(run): token usage per model and a price book from data`.

## Task 5: Caps

Spec §5.5, D6, R-g. AC: TU-03.4.

**Files:**
- Create: `crates/loom-run/src/caps.rs`

**Interfaces:**
- Consumes: `Usage`, `TokenKind`, `CapKind`, `StopCause` (Task 1), `PriceBook`, `UsageLedger`
  (Task 4).
- Produces: `Caps { input_tokens, output_tokens, cache_write_tokens, cache_read_tokens:
  Option<u64>, spend_usd: Option<f64>, wall_time: Option<Duration> }` (`Deserialize`, all
  optional) and `Caps::check(&self, ledger: &UsageLedger, prices: Option<&PriceBook>, elapsed: Duration) -> Option<StopCause>`.
  Order: wall time, then the four token kinds in the order above, then spend. A token cap is
  crossed when the total is **greater than** the cap. Spend is computed only when `spend_usd` is
  set; a `NoPrice` then becomes `StopCause::NoPrice` (R-g); no price book with a spend cap set is
  `NoPrice { model }` for the first model used.

- [ ] **Step 1: Write the failing tests**: `// AC: TU-03.4` `each_token_kind_has_its_own_cap`
  (four cases: just over each cap → `Cap(Tokens(kind))`; at the cap → `None`);
  `// AC: TU-03.4` `spend_over_its_cap_stops`; `no_spend_cap_needs_no_prices` (unknown model,
  no spend cap → `None`); `// AC: TU-03.4` `wall_time_over_its_cap_stops`;
  `no_caps_never_stop`.
- [ ] **Step 2: Run** `cargo nextest run -p loom-run caps` — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** — PASS.
- [ ] **Step 5: Commit** `feat(run): a cap per token kind, spend and wall time`.

## Task 6: Configuration, the per-run clone and the kit

Spec §5.2, §5.3 steps 2–3, D3, R-j. AC: TU-03.1 (the clone and the kit; the container side is
Task 8).

**Files:**
- Modify: `crates/loom-run/Cargo.toml` (`toml`, `tokio`), `src/lib.rs`
- Create: `crates/loom-run/src/config.rs`, `src/git.rs` (runs `git` through `tokio::process`,
  errors carry the command and stderr), `src/workspace.rs`, `src/kit.rs`, `tests/workspace.rs`,
  `tests/support/mod.rs` (makes throwaway git repos in a `tempfile` dir)

**Interfaces:**
- Produces:
  - `RunConfig` (spec §5.2: `data_dir`, `podman` default `"podman"`, `image`, `proxy_image`,
    `egress_network` default `"loom-egress"`, `blocked_ranges: Vec<IpNet>`-like strings,
    `credentials: Vec<String>` (variable names), `prices { url, overrides }`,
    `keep_run_dirs: bool`), `RunConfig::from_toml(&str) -> Result<RunConfig, ConfigError>`.
    Unknown keys are errors (`deny_unknown_fields`).
  - `RunSpec { story_key, repo_url, story_branch, default_branch, kit: KitRef { repository,
    version }, prompt, model: Option<String>, extra_args: Vec<String>, caps: Caps }`.
  - `Workspace::prepare(config: &RunConfig, run: RunId, spec: &RunSpec) -> Result<Workspace, WorkspaceError>`:
    mirror at `<data_dir>/mirrors/<sha256 of repo_url, 16 hex>.git` (`git clone --mirror` once,
    then `git fetch --prune` under the per-mirror lock), clone with `git clone --local <mirror>
    <data_dir>/runs/<run>/work`, `git switch <story_branch>` or
    `git switch -c <story_branch> origin/<default_branch>`, then
    `git remote set-url origin <repo_url>`. `Workspace::path()`, `Workspace::remove(self)`.
  - `Kit::prepare(config, kit: &KitRef) -> Result<Kit, KitError>`: `<data_dir>/kits/<sha256 of
    repository, 16 hex>/<version>` via `git clone --depth 1 --branch <version>`, once; after it,
    the checkout's files are made read-only. `Kit::path()`.

- [ ] **Step 1: Write the failing tests** (`tests/workspace.rs`, run on every platform):
  `// AC: TU-03.1` `two_runs_of_one_story_get_two_clones` (two `prepare` calls for one spec, run
  concurrently with `tokio::join!`: different paths, both on the story branch, a commit made in
  one isn't in the other); `a_missing_story_branch_starts_from_the_default_branch`
  (and `git remote get-url origin` equals `repo_url`); `// AC: TU-03.1`
  `the_kit_is_checked_out_at_its_pinned_version` (a kit repo with tags `v1` and `v2` and a
  `VERSION` file that differs: `prepare(v1)` holds v1's file); `a_kit_version_is_cloned_once`;
  `remove_deletes_the_run_dir`; in `config.rs`, `an_unknown_config_key_is_an_error`.
- [ ] **Step 2: Run** `cargo nextest run -p loom-run --test workspace` — FAIL.
- [ ] **Step 3: Implement** (`sha2` is already a workspace dependency).
- [ ] **Step 4: Run** — PASS on Windows and Linux (`git` paths with `\` on Windows).
- [ ] **Step 5: Commit** `feat(run): a fresh local clone per run and the pinned kit`.

## Task 7: The fake CLI, the launcher seam and the runner

Spec §5.3 steps 5–8 (without Podman), R-f, R-h, R-k, R-o, R-p. AC: TU-03.2, TU-03.4, TU-03.6.

**Files:**
- Create: `crates/loom-run/examples/fake_claude.rs`, `src/launcher/mod.rs`,
  `src/launcher/local.rs`, `src/chunks.rs`, `src/runner.rs`, `tests/local_runs.rs`,
  `tests/support/fake.rs` (finds the fake: `current_exe()`'s `deps/` parent, then
  `examples/fake_claude[.exe]`; a clear error if it isn't built). Cargo builds a package's
  examples when it builds its tests (`cargo test`, nextest and `cargo llvm-cov` alike), so no
  extra build step is needed; check that once in CI's `coverage` job too.

**Interfaces:**
- Consumes: Tasks 1–6.
- Produces:
  - The fake CLI. `update` → exit 0 (or exit 1 when `FAKE_UPDATE_FAILS=1`); `--version` →
    prints `9.9.9 (Fake)`; `-p …` → reads stdin, then acts by `FAKE_MODE`:
    `replay` (prints the file `FAKE_STREAM` line by line, `FAKE_DELAY_MS` between lines, writes
    `FAKE_STDERR` text to stderr), `sleep` (prints one `system/init` line, then sleeps for an
    hour), `spawn` (starts two children of itself in `sleep` mode, then sleeps),
    `report` (prints one JSON line `{"type":"fake_report","uid":…,"cwd":…,"branch":…,"kit_version":…,"args":[…],"stdin":…,"env":{…}}`
    — `uid` from `/proc/self/status` on Linux, `null` elsewhere; `branch` from `.git/HEAD`;
    `kit_version` from `$KIT_DIR/VERSION`; `env` holds the variables whose names start with
    `LOOM_TEST_`),
    `request` (HTTP GET `FAKE_URL` through `HTTP_PROXY` with `std::net::TcpStream`, prints
    `{"type":"fake_request","status":…}` or `{"type":"fake_request","error":…}`),
    `connect` (direct TCP connect to `FAKE_ADDR`, prints `{"type":"fake_connect","ok":…}`),
    `serve` (HTTP server on `0.0.0.0:FAKE_PORT` answering 200 `ok`, for Task 9).
    Plain `std` only; it is test tooling, not shipped code.
  - `trait Launcher: Send + Sync { fn start(&self, ctx: &LaunchContext) -> impl Future<Output = Result<Box<dyn Session>, LaunchError>> + Send; }`
    `LaunchContext { run: RunId, workspace: PathBuf, kit: PathBuf, env: Vec<String> (names), }`.
    `trait Session: Send { async exec(&mut self, args: &[String], stdin: Option<String>) -> Result<Exec, LaunchError>; async stop(&mut self) -> Result<(), LaunchError>; fn image_id(&self) -> Option<String>; }`
    (boxed futures as needed for `dyn`). `Exec { stdout: impl AsyncRead, stderr: impl AsyncRead, wait() -> Result<Option<i32>> }`.
  - `LocalLauncher { program: PathBuf }`: runs `program` with `cwd = workspace`, `KIT_DIR = kit`,
    `kill_on_drop(true)`; `stop` kills the running exec.
  - `ChunkWriter<S: RawStreamStore>`: buffers bytes per stream, appends when 64 KiB are buffered
    or 1 s has passed since the first unflushed byte, and on `finish()` (R-h).
  - `Runner::new(config: RunConfig, launcher: L, store: S, prices: Option<PriceBook>)`;
    `Runner::start(&self, spec: RunSpec) -> Result<(RunHandle, mpsc::Receiver<RunEvent>), RunError>`;
    `RunHandle { id: RunId, cancel(&self), join(self) -> StopCause }`.
    Sequence: `Workspace::prepare`, `Kit::prepare` → `Kit` event, `launcher.start` → `Image`
    event if any, `exec(["update"])` (failure → `CliUpdateFailed`, go on, R-f),
    `exec(["--version"])` → `CliVersion`, then `exec(["-p", "--output-format", "stream-json",
    "--verbose", "--dangerously-skip-permissions", "--plugin-dir", <kit>, (--model m), extra…])`
    with the prompt on stdin (R-k). Each stdout line → `ChunkWriter` (raw bytes, newline kept)
    and `parse_line` → event out; assistant usage → `UsageLedger`; `RateLimited` / `AuthFailed`
    → stop at once with that cause (R-o); `Caps::check` after each event and on a 250 ms timer.
    Stderr → `ChunkWriter` as stderr. On the end: `session.stop()`, `ChunkWriter::finish()`,
    the `SpendMismatch` check against the last `Result.total_cost_usd` (more than 5 % apart),
    `Workspace::remove` unless `keep_run_dirs`, and `RunEnded { cause }` as the last event.
    A cancel → `Cancelled`; exit 0 → `Completed`; else `Failed { exit_code }`.

- [ ] **Step 1: Write the failing tests** (`tests/local_runs.rs`, every platform, each with a
  `TestDb` and a `PgRawStreamStore`):
  `// AC: TU-03.2` `a_replayed_run_yields_typed_events_and_its_raw_stream` (fake replays
  `normal_run.jsonl` plus stderr text; the events after the prelude match `parse_line` of each
  line in order; `store.read(run, Stdout)` equals the fixture file's bytes exactly, `Stderr`
  equals the stderr text; the last event is `RunEnded { Completed }`);
  `// AC: TU-03.2` `the_cli_is_updated_and_its_version_recorded` (`CliVersion { "9.9.9 (Fake)" }`
  comes before the first stream event); `a_failed_update_still_runs` (`CliUpdateFailed`, then
  `Completed`); `the_prompt_goes_on_stdin` (`report` mode: `stdin` equals the prompt, `args`
  holds `--dangerously-skip-permissions` and the kit path);
  `// AC: TU-03.4` `a_token_cap_stops_the_run_with_its_cause` (cache-read cap below the
  fixture's cache reads → `RunEnded { Cap(Tokens(CacheRead)) }` and nothing after it);
  `// AC: TU-03.4` `a_spend_cap_stops_the_run`; `// AC: TU-03.4`
  `a_model_without_a_price_stops_a_run_with_a_spend_cap`;
  `// AC: TU-03.4` `a_silent_run_stops_at_its_time_cap` (`sleep` mode, `wall_time` 2 s →
  `Cap(WallTime)` within 4 s);
  `// AC: TU-03.6` `a_rate_limit_ends_the_run_as_rate_limited` (fake replays
  `rate_limit_rejected.jsonl` then sleeps → `RunEnded { RateLimited { resets_at } }` at once,
  not `Failed`); `// AC: TU-03.6` `an_auth_failure_ends_the_run_as_auth_failed`;
  `a_cancelled_run_ends_as_cancelled`; `a_spend_mismatch_is_recorded`;
  in `chunks.rs`, `chunks_flush_at_64_kib_and_after_a_second` (with `tokio::time::pause`).
- [ ] **Step 2: Run** `cargo nextest run -p loom-run --test local_runs` — FAIL.
- [ ] **Step 3: Implement** the fake, the seam, `ChunkWriter` and `Runner`. Lines are split on
  `\n` with `tokio::io::BufReader::read_until`, so bytes that aren't UTF-8 stay as they are.
- [ ] **Step 4: Run** — PASS on Windows and Linux; clippy — PASS.
- [ ] **Step 5: Commit** `feat(run): the runner, with a local launcher and the fake CLI`.

## Task 8: Podman: rootless container, cancel, clean-up

Spec §5.3 steps 1, 5, 7, R-a, R-l, R-n, and the plan's rulings on the container's life and
credentials. AC: TU-03.1, TU-03.3. Linux only.

**Files:**
- Create: `crates/loom-run/src/podman.rs` (the CLI wrapper), `src/launcher/podman.rs`,
  `tests/containers.rs` (`#![cfg(target_os = "linux")]`), `tests/support/podman.rs` (pins the
  test image `docker.io/library/debian:trixie-slim@sha256:<digest at build time>`, labels, and a
  guard that removes everything labelled with the test's run id on drop)
- Modify: `src/launcher/mod.rs`, `src/lib.rs` (`#[cfg(target_os = "linux")] pub mod podman;`)

**Interfaces:**
- Consumes: `Launcher`, `Session`, `LaunchContext` (Task 7).
- Produces:
  - `Podman::new(program)`, with `is_rootless()` (`podman info --format {{.Host.Security.Rootless}}`),
    `run_detached(args) -> container id`, `exec(…)`, `stop(id, time)`, `rm_force(id)`,
    `image_id(image)`, `network_create_internal(name)`, `network_rm(name)`, `logs_follow(id)`,
    `ls_by_label(label)`.
  - `PodmanLauncher::new(config: &RunConfig)`, implementing `Launcher`. `start`: refuses with
    `LaunchError::NotRootless` unless rootless; `podman run -d --init --rm --label loom.run=<id>
    --userns=keep-id --user <uid>:<gid> --network <net> -v <workspace>:/work:Z -w /work
    -v <kit>:/kit:ro,Z --env HTTP_PROXY=… --env HTTPS_PROXY=… --env NAME… <image> sleep infinity`;
    `exec` = `podman exec -i <id> claude …`; `stop` = `podman stop --time 3`, then `rm --force` if
    still there, within 10 s in all. Until Task 9 the network is a plain `--internal` network of
    the run; Task 9 adds the proxy. `image_id()` from `podman image inspect`.

- [ ] **Step 1: Write the failing tests** (`tests/containers.rs`; the fake binary is mounted at
  `/usr/local/bin/claude:ro`; the run config's `image` is the pinned Debian):
  `// AC: TU-03.1` `a_run_starts_in_a_rootless_container_with_its_clone_and_kit` (`report` mode:
  `uid` is not 0, `cwd` is `/work`, `branch` is the story branch, `kit_version` is the pinned
  one; the `Image` event holds Podman's id for the image);
  `// AC: TU-03.1` `two_concurrent_runs_of_one_story_never_share_a_worktree` (two runs at once:
  two containers, two clone paths, each container sees only its own `/work`);
  `// AC: TU-03.1` `a_podman_that_is_not_rootless_is_refused` (a `podman` stand-in script in a
  temp dir that prints `false` for the `info` query → `NotRootless`, nothing started);
  `// AC: TU-03.3` `cancelling_stops_the_process_tree_and_the_container_within_10_s` (`spawn`
  mode; wait until the container runs; cancel; assert `RunEnded { Cancelled }`, elapsed < 10 s,
  and `podman ps -a --filter label=loom.run=<id>` and `podman network ls` with the same filter
  are empty); `credentials_pass_by_name_never_by_value` (a variable `LOOM_TEST_SECRET=s3cr3t-…`
  is visible inside the container through `report`'s env dump, and the string `s3cr3t-` appears
  in no podman argument the launcher logged and in no event); `a_failed_start_leaves_nothing_behind`
  (an image that doesn't exist → an error, and no container or network with the label is left).
- [ ] **Step 2: Run** `cargo nextest run -p loom-run --test containers` on Linux — FAIL. (On
  Windows the file compiles to nothing; that is by design, D2.)
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** on Linux (WSL with `podman` installed, or push and read CI's `test` job) —
  PASS. Never claim a pass without the output.
- [ ] **Step 5: Commit** `feat(run): rootless Podman runs, cancel and clean-up`.

## Task 9: The egress proxy

Spec §5.3 step 4 and 6 (Egress events), R-b, R-c, D4, and the plan's rulings on the egress
network and ports. AC: TU-03.5. Linux only (the config generator is plain code and is tested
everywhere).

**Files:**
- Create: `crates/loom-run/src/egress/mod.rs` (Linux only: network and proxy lifecycle, log
  reading), `src/egress/squid_conf.rs` (every platform: the config text), `src/egress/access_log.rs`
  (every platform: one log line → `RunEvent::Egress`), `tests/egress.rs`
  (`#![cfg(target_os = "linux")]`)
- Modify: `src/launcher/podman.rs` (start the proxy before the run container, remove both and the
  network on stop), `tests/support/podman.rs` (Squid image `docker.io/ubuntu/squid@sha256:<digest>`)

**Interfaces:**
- Produces:
  - `squid_conf(blocked: &[String]) -> String`: `http_port 3128`; one `acl loom_blocked dst …` with
    the default ranges of spec §5.3 step 4 plus `::ffff:0:0/96` (IPv4-mapped), `224.0.0.0/4`,
    `240.0.0.0/4`, `ff00::/8` and `blocked`; `http_access deny loom_blocked`;
    `http_access allow all`; `cache deny all`; `logformat loom %ts %Ss %>Hs %rm %ru`;
    `access_log stdio:/dev/stdout loom`.
  - `parse_access_line(line: &str) -> Option<RunEvent>`: `Egress { host, port, allowed }`,
    `allowed = false` for `TCP_DENIED`.
  - The run's network `loom-run-<id>` (`--internal`); the proxy container `loom-proxy-<id>` on it
    and on `config.egress_network` (created if missing), with the generated config mounted at
    `/etc/squid/squid.conf:ro`; `HTTP_PROXY`/`HTTPS_PROXY` = `http://loom-proxy-<id>:3128`; its
    log followed into `Egress` events.

- [ ] **Step 1: Write the failing tests**: unit tests `the_config_blocks_every_private_range`
  (each default range and an extra one are in the `dst` acl; `deny` comes before `allow`) and
  `a_denied_line_is_a_blocked_egress`, `an_allowed_connect_names_host_and_port`. In
  `tests/egress.rs`, with a test egress network `198.18.N.0/24` and a `serve` container of the
  fake on it, and a second `serve` container on a private `10.N.0.0/24` network:
  `// AC: TU-03.5` `a_request_to_a_private_address_is_blocked_and_logged` (`request` mode to the
  private server through the proxy → status 403, and an `Egress { allowed: false }` event names
  its address); `// AC: TU-03.5` `a_request_to_a_public_address_goes_through_and_is_logged`
  (status 200, `Egress { allowed: true }`); `// AC: TU-03.5`
  `a_direct_connection_without_the_proxy_fails` (`connect` mode → `ok: false`).
- [ ] **Step 2: Run** on Linux — FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** on Linux — PASS; and Task 8's tests still pass with the proxy in place.
- [ ] **Step 5: Commit** `feat(run): public web only, through a logging proxy`.

## Task 10: End to end, documents and the gate

AC: all of TU-03 (one full container run).

**Files:**
- Create: `crates/loom-run/tests/full_run.rs` (`#![cfg(target_os = "linux")]`)
- Modify: `crates/loom-run/README.md` (what a run does; the image contract of R-n: non-root user,
  `git`, `sh`, `sleep`, the CLI by the native installer under that user's home; the config keys;
  how to run the container tests in WSL with Podman), `crates/loom-core/README.md`,
  `crates/loom-store/README.md`, `docs/trial/P03-run-runtime.md` (step 2's numbers)

- [ ] **Step 1: Write the failing test** `// AC: TU-03.1, TU-03.2, TU-03.5`
  `a_full_run_in_a_container_streams_stores_and_ends` (Podman launcher with proxy, `replay` of
  `normal_run.jsonl`: the typed events arrive, the raw stream in PostgreSQL equals the fixture,
  `RunEnded { Completed }`, and nothing labelled with the run is left).
- [ ] **Step 2: Run** on Linux — FAIL until the pieces are wired; then fix the wiring — PASS.
- [ ] **Step 3: Merge `origin/main`** (see "Order and merges" at the top), resolve `Cargo.toml` and
  `Cargo.lock`, and rerun every test.
- [ ] **Step 4: Run the full gate** `cargo xtask gate` on Windows (container tests left out by
  design) and read every line; then push, and read CI's `gate`, `test`, `coverage` and `ui`
  jobs: `test` and `coverage` run the container tests; `loom-run`, `loom-core` and `loom-store`
  are at or above the 80 % floor; `ac-trace` shows every TU-03 criterion tagged.
- [ ] **Step 5: Open the PR** `feat(run): the run runtime (P03)` with the template filled, every
  `Ruling:` listed, and no auto-merge yet (the review step decides). Record step 2's usage in the
  trial record.
