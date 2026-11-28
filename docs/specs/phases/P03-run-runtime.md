# P03 · Run runtime — phase spec

Status: draft for Jann's approval (decider approval, trial step 1) · 28 Nov 2026
Source: `docs/specs/00-umbrella.md` §9 P03, read at umbrella **version 8** (#10: T4 opened to the
public web, new T14). Scope and acceptance criteria come from there; this spec decides how.

## 1. Goal

After P03, `loom-run` can run one Claude Code CLI session for a story, safely and observably:

- in a **rootless Podman container** of the configured image, with its **own fresh local clone**
  of the component repo on the story branch, and the **workflow kit** at its pinned version;
- on a network that reaches **public HTTP(S) only**, through a per-run proxy that records every
  destination, never the home LAN or a private address range (T4, version 8);
- with the CLI **updated to the latest version** at start, and its version recorded (T14);
- turning the CLI's streamed JSON into **typed run events**, while the **raw stream** goes to
  PostgreSQL and is kept for 30 days (T7);
- stopped by **cancellation** (whole process tree and container within 10 s), by a **time cap**,
  by a **cap per token kind** or by a **spend cap**, with the cause recorded;
- telling **rate limits** and **authentication failures** apart as their own typed events.

Nothing schedules runs yet (P05, P08), and no state machine moves (P02). P03 is the engine a later
phase drives.

## 2. Acceptance criteria (verbatim from the umbrella)

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

**Reading of the criteria** (decided with Jann, never a change to them):
- *"fresh git worktree"* (TU-03.1) is a fresh working tree of git: a per-run local clone, not a
  `git worktree` (D3).
- *"the destinations of T4"* (TU-03.5) are those of T4 in umbrella version 8: public HTTP(S)
  destinations through the proxy; the home LAN, the database VM and every other private range
  are blocked (D4).
- *"Token … caps"* (TU-03.4) are one optional cap per token kind; a spend cap computed from the
  kinds' prices comes on top (D6).

## 3. Scope

**In:** `loom-run` (runtime, containers, clones, kit, egress proxy, stream parsing, usage, prices,
caps, cancellation); the run-event types and the raw-stream interface in `loom-core`; the first
table and migration in `loom-store` (raw streams; the price-list copy); behaviour tests against
real Podman and a throwaway PostgreSQL; the fake CLI and the fixtures they use; crate READMEs.

**Out:** scheduling, retries, stuck detection and budget windows (P05, P08); the run's state in the
state machines and its events in PostgreSQL (P02, P05); the real worker image with the toolchains
(P05 builds its first one, P19 ships it); the decider, `ask_decider` and the MCP tools (P04, P05,
P10, P11); the daily job that refreshes prices and prunes streams (the service, P05 or P19; P03
provides the functions); a `loom` CLI command for runs; a local throwaway-database helper in
`cargo xtask` (P02, as planned); Windows support for the runtime (decision 0004).

**Order.** P03 runs before P02, against `CLAUDE.md`'s stage-1 order (decision 0010). P01 is being
refined in parallel. Both add workspace dependencies, so the root `Cargo.toml` and `Cargo.lock`
may need a merge of `main` before P03's PR.

## 4. Decisions

### 4.1 Decided by Jann (refinement, 28 Nov 2026)

| | Decision |
|---|---|
| D1 | **P03 before P02** is allowed, against the stage-1 order of `CLAUDE.md`. Decision 0010. |
| D2 | **Podman only**, rootless. The container code and its tests are built on Linux only (`cfg(target_os = "linux")`); CI runs them, the local gate on Windows leaves them out. Nothing to install on Windows. Decision 0011. |
| D3 | **A local clone per run** is the run's fresh worktree: cloned from a shared mirror of the component repo with `git clone --local` (hard-linked objects), so it is fast, small and isolated from every other run. Decision 0012. |
| D4 | **Public web, home LAN blocked.** Runs reach any public HTTP(S) destination through a proxy that records every destination; the home LAN, the database VM and every private range stay blocked. Research tools need it. T4 changed in its own PR (#10). Decision 0013. |
| D5 | **Container tests pull images**, each pinned by digest; they need no other network. `CONTRIBUTING.md` §4 says so (#10). Decision 0014. |
| D6 | **A cap per token kind** (input, output, cache write, cache read), each optional, **plus a spend cap** per run computed from each kind's price per model. Decision 0015. |
| D7 | **New models and CLI versions need no code change** (T14): prices are data (a public price list fetched while Loom runs, overridden by Loom's config); every run updates the CLI before it starts and records the version. Decision 0016. |
| D8 | **The raw stream goes to PostgreSQL**, in P03 itself (Jann left the choice to Claude between P03 and a follow-up in P02; P03 proves TU-03.2 against the real store). Decision 0017. |
| D9 | **Sign-in by a `claude setup-token` token**, passed as an environment variable from Loom's secret store, never a copied credentials file. **Runs use `--dangerously-skip-permissions`**: the container is the sandbox, and questions go to the decider, not to permission prompts. Decision 0018. |

### 4.2 Claude's calls (Rulings, for Jann to check)

- **R-a Podman through its CLI**, not its REST API: the same way Loom uses `git` (T5), nothing to
  keep running, and every call can be copied into a shell to debug. `tokio::process` runs it.
- **R-b One proxy container per run**, on the run's own internal network: its log is the run's
  log, and cancelling a run removes its proxy and network with it. Costs about a second per run.
- **R-c Squid** is the proxy (directive 2): it filters by the *resolved* address (`dst` ACLs), so
  a public name that resolves to a private address is blocked too, and its access log names every
  destination. Its image is pinned by digest in the run config.
- **R-d Typed events live in `loom-core`** (domain types: P02's events, P13's SSE stream and P05
  use them); `loom-run` produces them; `loom-store` stores raw streams through a trait defined in
  `loom-core`, so `loom-run` never depends on `loom-store`.
- **R-e Unknown stream events are kept**, as `RunEvent::Unknown` with their raw JSON, never
  dropped and never an error: a new CLI version (T14) must not break a run.
- **R-f A failed CLI update doesn't stop the run.** It runs on the image's CLI, and the run records
  a `CliUpdateFailed` event with the version it used. A stale CLI is better than no run; the
  recorded version shows it.
- **R-g A model with no price stops the run** with the cause `NoPrice { model }`, but only when a
  spend cap is set. Without a spend cap, prices aren't needed and nothing stops.
- **R-h Raw streams are stored as byte chunks**, flushed every 64 KiB or every second, whichever
  comes first, so a crash loses at most about a second. PostgreSQL's TOAST compresses them; no
  compression crate.
- **R-i Runtime `sqlx` queries** (`sqlx::query`, not the `query!` macros), so building Loom never
  needs a database or an offline query cache.
- **R-j The run directory is removed when the run ends**, unless the run config keeps it. P05
  decides what a retry needs ("from a clean worktree", TU-05.4).
- **R-k The prompt goes to the CLI on stdin**, so its length is never limited by the command line.
- **R-l The image is any reference the config names**; the run records the image id Podman
  resolved, so a failure traces to the exact image. A digest is recommended, not enforced.
- **R-n The image contract.** The CLI runs as a non-root user inside the container
  (`--userns=keep-id --user <host uid>`, so the clone at `/work` is writable and owned by the
  caller), never as root, so `--dangerously-skip-permissions` needs no sandbox override. The
  image provides `git`, `sh`, and the CLI installed by Claude's native installer under that
  user's home, so `claude update` can replace it (T14). `loom-run`'s README states this contract;
  P05 builds the first real image to it.
- **R-o Rate limits and authentication failures end the run at once**, with their own stop causes
  `RateLimited { resets_at }` and `AuthFailed`: Loom stops the run when the event arrives rather
  than letting the CLI wait or retry (umbrella §3.3: never retry in a tight loop; P08 resumes
  after the reset).
- **R-p A process seam.** The runner starts the CLI through a small launcher trait: a container
  launcher (Podman, Linux) and a local-process launcher. Parsing, usage, caps and stop causes
  (TU-03.2, TU-03.4, TU-03.6) are tested through the local launcher, on Windows too and fast;
  real containers prove TU-03.1, TU-03.3, TU-03.5 and one full run end to end, which keeps the
  `test` and `coverage` jobs inside their time limits without a workflow change.
- **R-m The test image is a pinned Alpine**, with a **fake `claude`** (a POSIX shell script in the
  fixtures, mounted into the container; the local-launcher tests use a fake that runs on Windows and Linux,
  its form left to the builder) that answers `update`, `--version` and `-p` by replaying a
  fixture stream; it can also sleep, start child processes or make a request, for the cancel,
  cap and network tests.

## 5. Design

### 5.1 Crates and modules

One job per module (directive 1). Each names its tool or crate (directive 2).

| Crate · module | Job | Tool or crate |
|---|---|---|
| `loom-core::run` | `RunId`, `RunEvent` and its payloads, `StopCause`, the `RawStreamStore` trait | `serde`, `uuid` |
| `loom-run::config` | `RunConfig` and `RunSpec` (below), loaded from TOML | `serde`, `toml` |
| `loom-run::podman` | thin wrapper over the `podman` CLI: `info`, `network create/rm`, `run`, `stop`, `rm`, `inspect`; Linux only | `tokio::process` |
| `loom-run::workspace` | the shared mirror per repo (`git clone --mirror`, then `git fetch`) and the per-run clone (`git clone --local`), story branch checked out or created from the default branch, push remote set to GitHub | `git` CLI (T5) |
| `loom-run::kit` | the kit at its pinned version: one read-only checkout per version, cached, mounted into the container and passed to the CLI with `--plugin-dir` | `git` CLI |
| `loom-run::egress` | the run's internal network, the Squid container and its generated config; reads Squid's access log into `Egress` events | Podman, Squid |
| `loom-run::stream` | one stdout line → one `RunEvent`; unknown types → `Unknown` (R-e) | `serde_json` |
| `loom-run::usage` | token counts per model and per kind, de-duplicated by message id (the CLI repeats a message's usage on each of its content events) | – |
| `loom-run::prices` | the price book: parses the public price list, applies the config's overrides, prices a usage | `serde_json`, an HTTP client for the fetch |
| `loom-run::caps` | compares usage, spend and elapsed time with the run's caps; returns the first `StopCause` | – |
| `loom-run::runner` | puts it together: prepare, start, read, stop, clean up; the cancel handle | `tokio`, `tokio-util` (`CancellationToken`) |
| `loom-store::raw_stream` | `RawStreamStore` on PostgreSQL; `prune_raw_streams(older_than)` | `sqlx` (runtime queries, R-i) |
| `loom-store::price_list` | the last good copy of the price list | `sqlx` |
| `loom-store` migrations | `0001_run_runtime.sql`, applied by `sqlx::migrate!` | `sqlx migrate` |

Only `podman`, `egress` and the parts of `runner` that call them are `cfg(target_os = "linux")`.
Everything else builds and is tested on Windows too: stream parsing, usage, prices, caps, the
workspace and kit logic, and the store.

The HTTP client for the price fetch must pass `cargo deny` as configured: `reqwest` with
`rustls` on the `ring` provider (`aws-lc-rs` carries an OpenSSL licence that `deny.toml` doesn't
allow), or `ureq`. `deny.toml` is not changed.

### 5.2 Configuration

`RunConfig` (per Loom installation; P05 decides where it lives):

- `data_dir`: mirrors, kit checkouts and run directories live under it.
- `podman`: path of the binary (default `podman`).
- `image`: the worker image (R-l). `proxy_image`: Squid, by digest.
- `blocked_ranges`: extra address ranges to block on top of the fixed private ones (for example
  the database VM's address, if it isn't in a private range).
- `credentials`: names of the environment variables to pass into the container from Loom's own
  environment (the Claude token, the GitHub token). Values never appear in the config, the
  events or the logs.
- `prices`: the price-list URL (default: LiteLLM's `model_prices_and_context_window.json` on
  GitHub) and an override table per model and kind.
- `keep_run_dirs` (default false, R-j).

`RunSpec` (per run): the story id, the component repo URL, the story branch, the default branch,
the kit (repository and version), the prompt, the model and extra CLI arguments, and the caps:
`input_tokens`, `output_tokens`, `cache_write_tokens`, `cache_read_tokens`, `spend_usd`,
`wall_time`, each optional.

### 5.3 One run, step by step

1. **Check.** `podman info` must report rootless; otherwise the run refuses to start with a typed
   error. Nothing runs as root.
2. **Clone.** Fetch the mirror, clone it with `--local` into `<data_dir>/runs/<run-id>/work`,
   check out the story branch (created from the default branch if it doesn't exist yet), and set
   the push remote. Two runs of one story get two clones (TU-03.1).
3. **Kit.** Make sure the checkout of the kit's version exists (cloned once, read-only after).
4. **Network.** Create the run's internal network (`podman network create --internal`), start
   Squid on it and on the default network, with a config that allows HTTP and `CONNECT` to any
   destination except the blocked ranges, checked against the resolved address (R-c). Blocked by
   default: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `100.64.0.0/10`, `127.0.0.0/8`,
   `169.254.0.0/16`, `0.0.0.0/8`, `::1/128`, `fc00::/7`, `fe80::/10`, plus `blocked_ranges`.
5. **Container.** `podman run --rm --init` on the internal network only, labelled with the run
   id, the clone mounted at `/work`, the kit read-only at `/kit`, `HTTP_PROXY` and `HTTPS_PROXY`
   set to the proxy, the credentials passed as environment variables. Its command updates the CLI
   (`claude update`), prints `claude --version`, then runs
   `claude -p --output-format stream-json --verbose --dangerously-skip-permissions
   --plugin-dir /kit …` with the prompt on stdin (R-k). The version becomes a `CliVersion` event,
   the resolved image id an `Image` event, the kit version a `Kit` event.
6. **Read.** Each stdout line goes to the raw stream store (R-h) and to the parser; each event
   updates the usage and is checked against the caps. Stderr goes to the raw store too, marked as
   stderr. Squid's access log becomes `Egress { host, port, allowed }` events.
7. **Stop.** The run ends when the CLI exits, when a cap is crossed, or when it is cancelled.
   Stopping: `podman stop --time 3`, then `podman rm --force` if anything is left; then the proxy
   and the network are removed. `--init` makes the stop reach every process in the container. The
   outcome is a `RunEnded { cause }` event: `Completed`, `Failed { exit_code }`, `Cancelled`,
   `Cap(<which>)`, `NoPrice { model }`, `RateLimited { resets_at }` or `AuthFailed` (R-o).
8. **Clean up.** The run directory is removed unless kept (R-j). At the end, Loom compares its
   computed spend with the CLI's own `total_cost_usd` and records a `SpendMismatch` event when
   they differ by more than 5 % (D7).

The runner exposes the events as a stream (a `tokio::sync::mpsc` receiver) and returns a cancel
handle. The caller (P05) decides where events go; P03's tests collect them.

### 5.4 Typed run events

From the CLI's stream (seen on version 2.1.322, 28 Nov 2026): `system` with subtypes such as
`init` (model, tools, `claude_code_version`, `apiKeySource`), `hook_started`, `hook_response`;
`assistant` and `user` messages (with `message.id`, `message.model`, `message.usage`);
`rate_limit_event` (`rate_limit_info.status`, `resetsAt`, `rateLimitType`); `result` (`subtype`,
`is_error`, `api_error_status`, `usage`, `modelUsage`, `total_cost_usd`).

- **Rate limit** (TU-03.6): a `rate_limit_event` whose status is not `allowed`, or a `result` with
  `api_error_status` 429, becomes `RunEvent::RateLimited { kind, resets_at }`. An `allowed` one
  becomes `RunEvent::RateLimitStatus`, which is only information.
- **Authentication** (TU-03.6): a `result` with `api_error_status` 401 or 403, or an
  authentication error message, becomes `RunEvent::AuthFailed { message }`.
- The two never share a type with each other or with `Failed`.

Fixtures (`crates/loom-run/tests/fixtures/streams/`): real recordings where they can be made on
demand (a normal run; an authentication failure with an invalid token), scrubbed of session and
account ids; synthetic ones, marked as synthetic in the file, for a rejected rate limit and for
cases that can't be triggered on demand.

### 5.5 Usage, prices and caps

- Usage is counted per model and per kind: `input_tokens`, `output_tokens`,
  `cache_creation_input_tokens` (split into its 5-minute and 1-hour parts when the CLI reports
  them, because they have different prices), `cache_read_input_tokens`. A message's usage counts
  once (by message id); the final `result` is used only for the cross-check.
- Each token cap compares with the sum of its kind over all models. The spend cap compares with
  the sum of `tokens × price` over all models and kinds.
- The price book: the price list (fetched by `prices::refresh`, the last good copy kept in
  PostgreSQL by `loom-store::price_list`), with the config's overrides on top. A fetch that fails
  or doesn't parse keeps the last good copy and logs a warning. A model is looked up by the name
  the CLI reports; missing → R-g.
- The time cap counts wall time from the container's start.
- Each cap is checked after every event, and the time cap also on a timer, so a silent run still
  stops on time.

### 5.6 The raw stream in PostgreSQL

Migration `0001_run_runtime.sql`:

- `run_stream_chunks(run_id uuid, seq int, stream text check (stream in ('stdout','stderr')),
  written_at timestamptz, body bytea, primary key (run_id, seq))`: `bytea`, because the stream is
  kept byte for byte and stderr may hold bytes that aren't valid UTF-8, with an index on `written_at`.
- `price_lists(fetched_at timestamptz primary key, source_url text, body jsonb)`.

`RawStreamStore` (trait in `loom-core`): `append(run_id, stream, chunk)` and `read(run_id)`, which
returns the stream in order. `prune_raw_streams(older_than)` deletes chunks older than the given
age; the service calls it daily with 30 days (T7).

These are the store's first tables. P02 designs the rest of the store and adds its migrations
after this one.

### 5.7 Tests and CI

- **Container tests** (Linux only, D2) use real rootless Podman (installed on GitHub's
  `ubuntu-latest` runners), a pinned Alpine with the fake CLI (R-m) and the pinned Squid. Each
  test labels its containers and networks with its run id and removes them, even when it fails.
- **Database tests** read `LOOM_TEST_DATABASE_URL` (CI's service database) and create a database
  of their own per test, dropped afterwards. Without the variable they fail with a message that
  says how to start one; they are never skipped. The crate README gives the one command for a
  throwaway PostgreSQL in Docker Desktop.
- **The network test** (TU-03.5) needs no internet: a test HTTP server in a container on a
  network with a public-looking subnet (`198.18.0.0/24`, a benchmarking range that isn't
  private) is reachable through the proxy; the same server on a private subnet, and the host's
  LAN address, are blocked; a direct connection that bypasses the proxy fails.
- **No workflow changes.** CI already has Podman on its runners and a PostgreSQL service in the
  `test` and `coverage` jobs.
- `loom-run`'s line coverage must reach the 80 % floor in CI (Linux, where the container code
  builds). On Windows the container code isn't built, so it isn't counted.

## 6. How each AC is proven

| AC | Test (behaviour tests in `crates/loom-run/tests/` unless noted) |
|---|---|
| TU-03.1 | A run starts in a rootless container (Podman reports rootless; the run's process has a non-zero uid inside the container, R-n) from the configured image, sees its own clone on the story branch at `/work` and the kit's pinned version at `/kit`. Two concurrent runs of one story: two clone paths, two containers, and a commit made in one is absent from the other. A non-rootless Podman (faked `info`) is refused. |
| TU-03.2 | A recorded stream replayed by the fake CLI: every line becomes a typed event in order, unknown types become `Unknown`, and the raw stream read back from PostgreSQL equals the CLI's output byte for byte (stdout and stderr). Unit tests per event type; `loom-store` tests for append, read and prune. |
| TU-03.3 | The fake CLI starts child processes and sleeps; cancelling ends the run with `Cancelled`, and within 10 s no container, process or network of the run is left (checked with `podman ps -a` and `podman network ls` by label). |
| TU-03.4 | For each token kind, a stream that crosses that kind's cap stops the run with `Cap(<kind>)`; a spend cap with known prices stops with `Cap(spend)`; a model without a price stops with `NoPrice` (R-g); a time cap stops a silent run with `Cap(time)`. The cause is in the `RunEnded` event. Unit tests for `caps`, `usage` and `prices`. |
| TU-03.5 | From inside a run: a request to a private address through the proxy is refused and recorded as `Egress { allowed: false }`; a request to the public-looking test server succeeds and is recorded as allowed; a direct connection without the proxy fails. Unit test of the generated Squid config. |
| TU-03.6 | Fixtures with a rejected rate limit, a 429 result and an authentication failure become `RateLimited`, `RateLimited` and `AuthFailed` events, never `Failed` and never each other; each ends the run at once with the stop cause of the same name (R-o). |

## 7. Interfaces other phases use

- `loom_run::Runner::start(spec) -> (RunHandle, impl Stream<Item = RunEvent>)`; `RunHandle::cancel()`.
- `loom_core::run::{RunId, RunEvent, StopCause, RawStreamStore}`: P02 stores run events and
  their state changes, P05 drives runs and retries, P08 reads `RateLimited` and usage, P13 sends
  run events over SSE, P17 reads usage and spend.
- `loom_store::{PgRawStreamStore, prune_raw_streams, price_list}` and the first migration.
- `loom_run::prices::refresh` and `prune_raw_streams`: called on a schedule by the service.

## 8. Risks

- **The stream format changes with a CLI update** (T14 makes this likely). Mitigation: unknown
  events are kept (R-e), fixtures are re-recorded when the format changes, and the run records its
  CLI version.
- **Rootless Podman on GitHub's runners** could change with a runner image update. If it breaks,
  the container tests fail loudly; they are never skipped.
- **Exfiltration over the open web** (decision 0013): limited by the GitHub token's scope and the
  log of every destination. DNS inside the internal network resolves only through Podman's
  resolver; any remaining DNS channel is accepted with the open web.
- **Price list drift**: a wrong or late price in the public list. Mitigation: config overrides and
  the `SpendMismatch` cross-check.
- **Merge with P01**: both add workspace dependencies; `Cargo.toml` and `Cargo.lock` may need a
  merge of `main` before P03's PR.
- **Decision numbers**: P01's refinement may add decisions in parallel; if numbers clash, the PR
  that merges second renumbers its own.

## 9. Left to the builder

- Exact type and field names, module file layout, and how the fake CLI is parameterised.
- The pinned image digests (Alpine, Squid, the test HTTP server) at the time of building.
- Which HTTP client (`reqwest` with `rustls` on `ring`, or `ureq`), as long as `cargo deny` passes
  unchanged.
- How the Squid access log is read (a mounted log file in the run directory or `podman logs`), as
  long as every destination becomes an `Egress` event.
- Recording the real fixtures and scrubbing them.
- The form of the local fake CLI (R-p), how usage is de-duplicated per message id (the last value
  of a message wins), locking the shared mirror against concurrent fetches, extra blocked ranges
  (IPv4-mapped IPv6, multicast, `240.0.0.0/4`), and how Squid reaches the test network.
