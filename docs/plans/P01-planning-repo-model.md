# P01 · Planning repo model — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to
> implement this plan task-by-task, in a fresh session (`CLAUDE.md`). Steps use checkbox
> (`- [ ]`) syntax for tracking.

**Goal:** `loom-plan` reads a planning repo (the BMad PO's ticket tree plus Loom's keys) into
typed models, `loom plan check <path>` reports its mistakes and unfinished ready work, directives
come out by layer, and a JSON schema for every front-matter type is generated and committed.

**Architecture:** Reading and checking are separate. `read(path)` walks the tree once, parses
every file into its model, and keeps every parse problem as a `Finding` (never an abort).
`check(&repo)` builds the shared inputs once (a key index and the dependency graph) into a
`check::Context`, then runs a registry of rules, one module each. The `loom` binary wraps both
behind `clap`.

**Tech Stack:** Rust 1.99.1 (edition 2024); `serde` + `toml` (workspace), `serde-saphyr` 2.2
(YAML front matter), `pulldown-cmark` 0.13 (workspace; bodies and the front-matter block),
`globset` (workspace), `schemars` 1.2, `petgraph` 0.8, `thiserror`, `tracing`; binary: `clap`,
`anyhow`, `tracing-subscriber`; dev: `jsonschema` 0.63 (`default-features = false`), `tempfile`.

**Spec:** `docs/specs/phases/P01-planning-repo-model.md` (approved by Jann, 28 Nov 2026). Read it
whole before Task 1; its §5 is the design and its rulings R-a to R-r bind every task.
Decisions 0020–0025. Umbrella §2.1 (version 9) is the layout. The format source (spec R-b) is the
`requirements-bmad` plugin 6.13.0-next, PO agent: on Jann's machine under
`~/.claude/plugins/synced/*/requirements-bmad~g2/` (`skills/method/bmad-agent-po/assets/`,
`.../scripts/tickets.py`, `tools/ticket-tree-rules.md`). Its upstream is
`https://github.com/mesoneer-ag/BMAD-METHOD`.

**Before Task 1:** the phase spec reaches `main` through its own spec PR (it is a protected
path). Merge `origin/main` into this branch first, so `docs/specs/phases/P01-planning-repo-model.md`
is here; `ac-trace` reads the phase's ACs from it.

---

## Files and areas P01 touches

None of these is a protected path (`.loom/protected.toml`): the root `Cargo.toml` is protected
only in its `[workspace.lints]` table, which P01 doesn't touch. **If `cargo deny` refuses a new
dependency's licence, stop and ask Jann**: `deny.toml` is protected and never changes in this PR.

| Area | Paths | Tasks |
|---|---|---|
| Workspace | `Cargo.toml` (`[workspace.dependencies]` only), `Cargo.lock` | 1 |
| Domain enums | `crates/loom-core/{Cargo.toml,src/lib.rs,src/state.rs}` | 1 |
| Reader and checks | `crates/loom-plan/{Cargo.toml,README.md}`, `crates/loom-plan/src/**` | 1–12 |
| Schemas | `crates/loom-plan/schemas/*.schema.json` | 12 |
| Fixtures and tests | `crates/loom-plan/tests/**` | 2–12 |
| CLI | `crates/loom/{Cargo.toml,README.md}`, `crates/loom/src/**`, `crates/loom/tests/**` | 13 |
| Trial | `docs/trial/P01-planning-repo-model.md` | 14 |
| Outside this repo | a PR to `FPGSchiba/theos-planning` (spec §5.10) | 14 |

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

Also binding (`CLAUDE.md`): no `unwrap`/`expect`/`panic!` outside tests (the workspace lints deny
them); no `println!`/`eprintln!` (write to a locked `stdout`/`stderr`); public items documented;
no `#[allow]`; small files with one job; Conventional Commits with `Phase: P01`, `Refs:` the ACs,
and the Claude trailers; the project's directive 7 (validate at the boundary) and directive 8 (no
panics on bad input, no silent fallbacks).

## Acceptance criteria (umbrella §9 P01, verbatim)

**TU-01 Reading and validating a planning repo**
- TU-01.1 `loom-plan` parses every file of §2.1 into typed models. Unknown front-matter keys are
  errors that name the file and the key.
- TU-01.2 `loom plan check <path>` reports broken references (unknown component, epic or
  story), dependency cycles, stories without ACs, epics without a goal, scope or epic-level
  ACs, and duplicate ids. An epic without stories is valid (it gets sliced). It exits non-zero on any
  of them.
- TU-01.3 Directives resolve by layer (project, then component). A component directive marked as
  contradicting a project directive is reported.
- TU-01.4 A JSON schema for every front-matter type is generated from the models and shipped,
  so the BMad fork's templates validate against it.

## Review Focus

Inputs the spec implies but no AC names; each has its test in the owning task.

1. **Windows line endings.** A file saved with CRLF reads exactly like its LF copy (front matter
   found, sections and criteria the same). Test in Task 3 and Task 6.
2. **A UTF-8 byte-order mark** before `---` still yields the front matter. Test in Task 3.
3. **Broken front matter**: an opening `---` with no closing one, an empty block, YAML that is a
   list instead of a map. Each is a `parse` finding with the file, never a panic. Test in Task 3.
4. **Binary and non-UTF-8 files** in `canon/` or `archive/` (images of the visual canon) become a
   `Document` without text and give no finding. Test in Task 7.
5. **Paths on Windows**: every `Finding.path` uses `/`, relative to the repo root, also when read
   on Windows; `.git/` and `_bmad/` are never read. Test in Task 7.

---

## Shared vocabulary (the types tasks hand to each other)

These are fixed here so every task uses the same names. Fields not listed are the implementer's.

```rust
// crates/loom-plan/src/finding.rs
pub enum Severity { Error, Note }
pub enum Rule { Parse, UnknownKey, Layout, Drift, BrokenReference, Cycle, DuplicateId,
                DirectiveContradiction, UnnamedDirective, EpicIncomplete, StoryWithoutAcs,
                MissingKey }            // Display: kebab-case, e.g. "unknown-key"
pub struct Finding { pub severity: Severity, pub rule: Rule,
                     pub path: String,      // repo-relative, '/' separators
                     pub line: Option<u32>, pub message: String }   // Serialize

// crates/loom-plan/src/model/ids.rs
pub struct Key(String);                 // ^[A-Z][A-Z0-9]*-[0-9]+$ ; serde try_from String
pub struct TicketId(String);            // ^[0-9A-Za-z]+$ ; deserializes from TOML/YAML int or string
pub enum AfterRef { Sibling(TicketId), Cross { epic: TicketId, entry: TicketId }, Epic(String) }
pub enum CriterionId { Story { key: Key, number: u32 }, Epic { key: Key, number: u32 } }
                                        // Display "ED-12.3" / "ED-4.R2"
pub enum LoomMark { Ready, Paused }     // "ready" | "paused"

// crates/loom-plan/src/lib.rs
pub fn read(root: &Path) -> Result<PlanningRepo, ReadError>;
pub fn check(repo: &PlanningRepo) -> Vec<Finding>;   // read findings + every rule, errors first
pub enum ReadError { NotADirectory(PathBuf), Io { path: PathBuf, source: std::io::Error } }
```

`PlanningRepo` (Task 7) holds: `root`, `settings: Option<LoomToml>`, `vision: Option<Vision>`,
`project_directives: Option<DirectiveFile>`, `components: Vec<Component>`,
`decisions: Vec<DecisionRecord>`, `state_mirrors: Vec<StateMirror>`, `documents: Vec<Document>`,
`read_findings: Vec<Finding>`. A `Component` holds `id`, `settings`, its two directive files,
`initiative: Option<Initiative>`, `extra_initiatives: Vec<String>` (for R-j), `backlog: Vec<Story>`.
An `Initiative` holds `slug`, `file`, `breakdown`, `epics`, `stories` (when it has no epics) and
`children`. An `Epic` holds `slug`, `path`, `file`, `body`, `row` (its `[[epic]]` table), `order`,
`breakdown`, `stories`. A `Story` holds `kind` (story, spike, bug), `id`, `entry`, `file` (front
matter plus criteria), `plan`, `order`, and has the accessors `key()`, `loom()`, `needs()`,
`after()`, `criteria()`, `is_dropped()`, each applying R-o (the file wins).

---

### Task 1: Dependencies and the state enums

**Files:**
- Modify: `Cargo.toml` (`[workspace.dependencies]`: add `serde-saphyr = "2.2"`,
  `schemars = "1.2"`, `petgraph = { version = "0.8", default-features = false, features = ["std", "graphmap"] }`,
  `jsonschema = { version = "0.63", default-features = false }`), `Cargo.lock`
- Modify: `crates/loom-core/Cargo.toml` (`serde`, `schemars`), `crates/loom-core/src/lib.rs`
- Create: `crates/loom-core/src/state.rs`
- Modify: `crates/loom-plan/Cargo.toml` (`loom-core` by path; `serde`, `serde-saphyr`, `toml`,
  `pulldown-cmark`, `globset`, `schemars`, `petgraph`, `thiserror`, `tracing`; dev:
  `jsonschema`, `serde_json`, `tempfile`)

**Interfaces:**
- Produces: `loom_core::state::{EpicState, StoryState}`, `Serialize + Deserialize + JsonSchema`,
  kebab-case. `EpicState`: `Intake, Slicing, Active, Blocked, Done`. `StoryState`: `Queued,
  Refining, Refined, Implementing, InReview, Blocked, Merged`.

- [ ] **Step 1: Write the failing test** in `state.rs`: `every_state_name_round_trips` asserts
  each variant serializes to its umbrella §3.2 name (`"in-review"` for `InReview`) through
  `serde_json` and back. No AC tag (it supports TU-01.1 through `StateMirror`).
- [ ] **Step 2:** `cargo test -p loom-core` → FAIL (module missing).
- [ ] **Step 3:** Add the dependencies and the enums.
- [ ] **Step 4:** `cargo test -p loom-core` → PASS; `cargo deny --locked check` → no error. A
  licence error: stop and ask Jann (see above).
- [ ] **Step 5: Commit** `build(core): state names and the dependencies of the planning reader`.

### Task 2: Findings and value types

**Files:**
- Create: `crates/loom-plan/src/finding.rs`, `crates/loom-plan/src/model/mod.rs`,
  `crates/loom-plan/src/model/ids.rs`
- Modify: `crates/loom-plan/src/lib.rs`

**Interfaces:**
- Produces: `Finding`, `Severity`, `Rule`, `Key`, `TicketId`, `AfterRef`, `CriterionId`,
  `LoomMark` as in the shared vocabulary. `Key: FromStr + TryFrom<String> + Display + Ord + Hash`.
  `AfterRef: TryFrom<TicketIdOrString>` parsing `3`, `"2.3"`, `"epic-<slug>"`. Each value type
  has a `JsonSchema` with its pattern (`#[schemars(regex(pattern = ...))]` or a manual impl).

- [ ] **Step 1: Write the failing tests** (unit, in `ids.rs`):
  `key_accepts_ED_12_and_rejects_ed_12_ED12_ED_x` (asserts `"ED-12".parse::<Key>()` is `Ok`,
  `"ed-12"`, `"ED12"`, `"ED-x"`, `""` are `Err`); `ticket_id_reads_int_and_string_alike`
  (TOML `id = 6` and `id = "6"` give equal `TicketId`s); `after_ref_forms` (`3` → `Sibling`,
  `"2.3"` → `Cross`, `"epic-cart"` → `Epic("epic-cart")`, `"x.y.z"` → `Err`);
  `criterion_ids_display` (`ED-12.3`, `ED-4.R2`); in `finding.rs`:
  `rule_names_are_kebab_case` (`Rule::UnknownKey` → `"unknown-key"`, `Rule::StoryWithoutAcs` →
  `"story-without-acs"`).
- [ ] **Step 2:** `cargo test -p loom-plan` → FAIL.
- [ ] **Step 3:** Implement. Errors of the value types are one `thiserror` enum `IdError` whose
  messages say the expected form (`a key is capital letters, a dash and a number, like ED-12`).
- [ ] **Step 4:** `cargo test -p loom-plan` → PASS.
- [ ] **Step 5: Commit** `feat(plan): findings and the ids of the planning repo`.

### Task 3: Parsing front matter and TOML into findings

**Files:**
- Create: `crates/loom-plan/src/parse/mod.rs`, `parse/front_matter.rs`, `parse/toml_file.rs`
- Test: unit tests in both files

**Interfaces:**
- Produces:
  - `pub(crate) fn split_front_matter(text: &str) -> Split<'_>` where `Split { front: Option<FrontBlock<'_>>, body: &str, body_line: u32 }`;
    found with `pulldown_cmark::Parser::new_ext(text, Options::ENABLE_YAML_STYLE_METADATA_BLOCKS)`
    (the `MetadataBlock(YamlStyle)` event and its byte range); a leading BOM is stripped first.
  - `pub(crate) fn front_matter<T: DeserializeOwned>(path: &str, text: &str) -> Result<(Option<T>, &str), Finding>`
    deserializing with `serde_saphyr::from_str`.
  - `pub(crate) fn toml_file<T: DeserializeOwned>(path: &str, text: &str) -> Result<T, Finding>`.
  - Both map serde's "unknown field `x`" to `Rule::UnknownKey` with message
    ``unknown key `x` in <path>`` (and the expected keys serde lists), everything else to
    `Rule::Parse`; `line` from the error's location when the crate gives one.

- [ ] **Step 1: Write the failing tests**, with a test struct `#[serde(deny_unknown_fields)] struct T { a: u32 }`:
  - `// AC: TU-01.1` `unknown_front_matter_key_names_file_and_key`: `"---\na: 1\nzz: 2\n---\nbody"`
    with path `x/y.md` → `Err(f)`, `f.rule == Rule::UnknownKey`, `f.message` contains `zz` and `x/y.md`.
  - `// AC: TU-01.1` `unknown_toml_key_names_file_and_key`: same for TOML `a = 1\nzz = 2`.
  - `no_front_matter_gives_none_and_the_whole_body`.
  - `crlf_front_matter_reads_like_lf` (Review Focus 1).
  - `bom_before_front_matter_is_ignored` (Review Focus 2).
  - `unclosed_or_non_map_front_matter_is_a_parse_finding` (Review Focus 3): unclosed `---`,
    `---\n---`, `---\n- a\n---` → `Rule::Parse` or `None` as documented on the function; never a panic.
- [ ] **Step 2:** `cargo test -p loom-plan parse::` → FAIL.
- [ ] **Step 3:** Implement. If `serde-saphyr` doesn't name the unknown key in its error, switch to
  `serde_norway` and write the switch as a `Ruling:` line in the PR (spec §8).
- [ ] **Step 4:** `cargo test -p loom-plan parse::` → PASS.
- [ ] **Step 5: Commit** `feat(plan): read front matter and TOML into typed values`.

### Task 4: Settings, document, decision and state models

**Files:**
- Create: `crates/loom-plan/src/model/{loom_toml.rs, component_toml.rs, document.rs, decision.rs, state_mirror.rs}`

**Interfaces:**
- Produces (all `Deserialize + JsonSchema + Debug + Clone`, `deny_unknown_fields`, keys of spec §5.2):
  - `LoomToml { format: Format, project: ProjectSettings { id, name }, budget: Option<Budget { weight: u32 }>, notify: Option<Notify { target: String }> }`
  - `Format` accepts only `1` (any other value is a parse error saying which formats Loom reads).
  - `ComponentToml { format: Format, component: ComponentSettings }` with `id, name, repo, default_branch, kit_version: Option<String>, forbidden_paths: Vec<String>, slicing: Slicing (Auto default | Review), spec_path: Option<String>, plan_path: Option<String>}`;
    `ComponentToml::validate(&self, path: &str) -> Vec<Finding>` checks each glob with `globset::Glob::new`.
  - `DocFront { status: Option<DocStatus /* only Draft */> }` — the front matter of `vision.md` and directive files.
  - `Document { path: String, front_matter: Option<String>, text: Option<String> }` (`text` is `None` for non-UTF-8 files).
  - `DecisionFront { kind: DecisionKind (Decision | Answer), date: String /* YYYY-MM-DD, checked */ }`; `DecisionRecord { number: u32, slug: String, path: String, front: DecisionFront }`.
  - `StateMirror { format: Format, epic: Option<EpicState>, stories: BTreeMap<Key, StoryState> }`.

- [ ] **Step 1: Write the failing tests** (`crates/loom-plan/tests/models.rs`, fixtures in
  `tests/fixtures/models/`): one test per model that a valid sample parses with the expected
  values, and `// AC: TU-01.1` `unknown_key_in_<model>_is_named` for `loom_toml`,
  `component_toml`, `doc_front`, `decision_front`, `state_mirror` (each asserts `Rule::UnknownKey`
  and the key in the message); `format_2_is_refused`; `bad_forbidden_glob_is_a_finding`;
  `decision_date_must_be_iso`.
- [ ] **Step 2:** `cargo test -p loom-plan --test models` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** → PASS.
- [ ] **Step 5: Commit** `feat(plan): settings, documents, decisions and state mirrors`.

### Task 5: Ticket-tree models

**Files:**
- Create: `crates/loom-plan/src/model/{initiative.rs, epic.rs, leaf.rs, plan_file.rs, breakdown.rs}`
- Create: `crates/loom-plan/tests/fixtures/po-templates/` — one filled copy per PO template
  (`initiative.md`, `epic.md`, `story.md`, `spike.md`, `bug.md`, `plan.md`, `tickets-epics.toml`,
  `tickets-entries.toml`, `tickets-initiatives.toml`), taken from the R-b sources with sample
  values in place of the `[...]` placeholders and Loom's keys added. Plain business sample
  content only (hard rule 7).

**Interfaces:**
- Produces the front-matter structs `InitiativeFront`, `EpicFront`, `LeafFront`, `PlanFront` and
  `Breakdown { next_id: Option<u32>, epic: Vec<EpicRow>, entry: Vec<EntryRow>, initiative: Vec<InitiativeRow> }`
  with exactly the keys of spec §5.2 (Loom keys typed: `key: Option<Key>`, `loom: Option<LoomMark>`,
  `needs: Vec<Key>`, `origin: Option<Origin /* Sliced */>`, `after: Vec<AfterRef>`).
  `EpicRow.after` takes ids or `{ epic, needs }` tables (an untagged enum). `PlanFront.status` is
  the PO's eight statuses (`draft`, `ready-for-dev`, `in-progress`, `in-review`, `built`, `done`,
  `blocked`, `dropped`). `LeafKind { Story, Spike, Bug }` from `type`.
  `Breakdown::kinds(&self) -> usize` (how many table kinds are non-empty).

- [ ] **Step 1:** Before writing the structs, read the R-b templates and `tickets-template.toml`
  and compare their keys with spec §5.2. A key there that §5.2 misses is added, as a `Ruling:`
  line for the PR.
- [ ] **Step 2: Write the failing tests** (`tests/ticket_models.rs`):
  `every_po_template_parses` (each `po-templates/` file into its struct);
  `// AC: TU-01.1` `unknown_key_in_<kind>_is_named` for initiative, epic, leaf, plan and
  `tickets.toml` (R-g); `bad_key_form_is_a_parse_finding` (`key: ed-1`);
  `loom_mark_accepts_ready_and_paused_only`.
- [ ] **Step 3:** `cargo test -p loom-plan --test ticket_models` → FAIL.
- [ ] **Step 4:** Implement. Flat structs, no `#[serde(flatten)]` (spec §8).
- [ ] **Step 5:** → PASS.
- [ ] **Step 6: Commit** `feat(plan): the PO's ticket tree as typed models`.

### Task 6: Bodies: sections, criteria, requirement lines, directives

**Files:**
- Create: `crates/loom-plan/src/parse/body.rs`, `crates/loom-plan/src/parse/directives.rs`,
  `crates/loom-plan/src/model/directive.rs`

**Interfaces:**
- Produces:
  - `pub struct Sections` with `fn get(&self, heading: &str) -> Option<&Section>` (`##` level,
    case-insensitive heading match); `Section { text: String, line: u32 }`; `fn is_empty()`
    ignores whitespace and HTML comments.
  - `pub struct Criterion { pub number: u32, pub name: Option<String>, pub text: String, pub line: u32 }`;
    `fn criteria(sections: &Sections, heading: &str) -> Vec<Criterion>`: items of the first
    ordered list under that heading, numbered from the list's start number.
  - `pub struct RequirementLine { pub number: u32, pub text: String, pub line: u32 }`;
    `fn requirement_lines(sections: &Sections) -> Vec<RequirementLine>`: list items or paragraphs
    under `Requirements` starting `R<n>`.
  - `EpicBody { goal_present: bool, scope_present: bool, requirements: Vec<RequirementLine> }`
    (goal = Description and Outcome non-empty; scope = Boundaries non-empty).
  - `DirectiveFile { path: String, front: Option<DocFront>, directives: Vec<Directive> }`;
    `Directive { name: Option<String>, kind: DirectiveKind (Engineering | Design), text: String, contradicts: Option<String>, line: u32 }`;
    `fn parse_directives(path: &str, text: &str, default_kind: Option<DirectiveKind>) -> Result<DirectiveFile, Finding>`
    (`None` for `project.md`: the `## Engineering` / `## Design` headings set the kind).
  - All built on `pulldown_cmark` events with offsets (line numbers from byte offsets).

- [ ] **Step 1: Write the failing tests** (unit):
  `story_criteria_are_the_numbered_items_under_requirements` (three Given/When/Then items →
  numbers 1–3, names from the bold span); `a_verify_line_alone_is_no_criterion`;
  `bug_criteria_come_from_acceptance_criteria`; `epic_requirement_lines_are_read`
  (`R1 (parent R2): …`, `R2: …` → 1, 2); `epic_goal_and_scope_presence`;
  `directive_names_drop_the_trailing_stop` (`**Quality over speed.**` → `Quality over speed`);
  `contradicts_line_is_read`; `project_headings_set_directive_kind`;
  `unnamed_directive_has_no_name`; `crlf_body_reads_like_lf` (Review Focus 1).
- [ ] **Step 2:** `cargo test -p loom-plan parse::` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** → PASS.
- [ ] **Step 5: Commit** `feat(plan): read criteria, requirement lines and directives`.

### Task 7: Reading the whole tree

**Files:**
- Create: `crates/loom-plan/src/read/{mod.rs, walk.rs, classify.rs, assemble.rs}`,
  `crates/loom-plan/src/repo.rs` (`PlanningRepo`, `Component`, `Initiative`, `Epic`, `Story`)
- Modify: `crates/loom-plan/src/lib.rs` (`read`, `ReadError`)
- Create: `crates/loom-plan/tests/fixtures/valid/` (spec §5.9: a made-up project with two
  components, `app` and `api`; one with a parent initiative and one child; epics draft, ready,
  paused and dropped; stories as files and as entries only; a spike; a bug in `backlog/`; a plan
  file; a cross-component `needs`; directives on both layers; two decision records; a `_state/`
  mirror; a README, a PRD companion, a `.memlog.md`, a PNG in `canon/`, a `_bmad/` folder with a
  deliberately invalid file inside)
- Test: `crates/loom-plan/tests/read.rs`

**Interfaces:**
- Consumes: Tasks 2–6.
- Produces: `read(root) -> Result<PlanningRepo, ReadError>` and the `PlanningRepo` shape of the
  shared vocabulary. Classification follows spec §5.1 and the PO's name patterns (`NAME_RE`,
  `EPIC_RE` of spec §5.1). Layout findings of R-n, R-j and the `component.toml` id are produced
  here (they are about where files are). `Story` accessors apply R-o; `drift` findings for
  disagreeing Loom keys are produced here too. `ReadError` only when `root` isn't a readable
  directory.

- [ ] **Step 1: Write the failing tests:**
  - `// AC: TU-01.1` `every_file_of_the_valid_tree_is_read_as_its_model`: asserts the settings,
    both components with their `component.toml`, the initiative with its child, each epic with
    its body, each story's kind, criteria ids (`<key>.<n>`), the plan joined to its story, the
    directives, decisions, the state mirror, and that README, PRD and `.memlog.md` are documents;
    `read_findings` is empty.
  - `// AC: TU-01.1` `unknown_key_anywhere_in_the_tree_is_reported_with_file_and_key`: a copy of
    `valid/` (with `tempfile`) with one unknown key added to an epic file → one `unknown-key`
    finding naming that path and key; reading still returns every other file.
  - `build_order_is_the_table_order` (stories' `order` follows `tickets.toml`, not ids).
  - `file_wins_over_entry` and `differing_loom_key_on_file_and_entry_is_drift` (R-o).
  - `binary_canon_file_is_a_document_without_text` (Review Focus 4).
  - `finding_paths_use_slashes_and_skip_git_and_bmad` (Review Focus 5): no finding mentions
    `_bmad`; every path is relative and contains no `\`.
  - `a_missing_root_is_a_read_error`.
- [ ] **Step 2:** `cargo test -p loom-plan --test read` → FAIL.
- [ ] **Step 3:** Implement: `walk` lists files (std `read_dir`, no symlink following, skipping
  `.git`, `_bmad` and dotfiles except `.memlog.md`); `classify` maps a relative path to a file
  kind; `assemble` builds the tree. One `tracing` span per file.
- [ ] **Step 4:** → PASS.
- [ ] **Step 5: Commit** `feat(plan): read a whole planning repo`.

### Task 8: The check registry, references and duplicates

**Files:**
- Create: `crates/loom-plan/src/check/{mod.rs, context.rs, references.rs, duplicates.rs}`,
  `crates/loom-plan/src/index.rs`
- Create: `crates/loom-plan/tests/fixtures/invalid/<rule>-<case>/` trees for this task's cases
- Test: `crates/loom-plan/tests/check_references.rs`

**Interfaces:**
- Produces:
  - `pub struct Index` built once from a `PlanningRepo`: keys → item (epic or story, component,
    path), epic ids and slugs per initiative, entry ids per breakdown, requirement-line numbers per
    epic, project directive names. Duplicate keys are kept (all occurrences) for the duplicate rule.
  - `pub(crate) struct Context<'a> { pub repo: &'a PlanningRepo, pub index: Index }`, built once
    in `check()`; Task 9 adds `pub graph: DependencyGraph`.
  - `pub(crate) type RuleFn = fn(&Context<'_>) -> Vec<Finding>;` and
    `pub(crate) const RULES: &[RuleFn]` in `check/mod.rs`; `pub fn check(repo) -> Vec<Finding>`
    returns `repo.read_findings` plus every rule's findings, errors before notes, then by path.
  - Rules: `references::check` (`after`, `needs`, `covers`, `parent`, plan `ticket`, `_state`
    component folder and epic key, every miss is `broken-reference` naming what is unknown and
    where); `duplicates::check` (`duplicate-id` for the five cases of spec §5.4).
- [ ] **Step 1: Write the failing tests**, each reading its `invalid/` tree and asserting exactly
  the expected finding (rule, path, a word of the message):
  `// AC: TU-01.2` `unknown_component_is_reported` (`_state/nope/ED-1.toml`),
  `unknown_epic_is_reported` (`after: [epic-nope]`), `unknown_story_is_reported`
  (`after: ["1.9"]`), `unknown_key_in_needs_is_reported`, `unknown_requirement_in_covers_is_reported`,
  `duplicate_key_is_reported`, `duplicate_entry_id_is_reported`,
  `duplicate_criterion_number_is_reported`, `duplicate_decision_number_is_reported`;
  `valid_tree_has_no_errors` (`check(&read(valid))` has no `Severity::Error`).
- [ ] **Step 2:** → FAIL. **Step 3:** Implement. **Step 4:** → PASS.
- [ ] **Step 5: Commit** `feat(plan): check references and duplicate ids`.

### Task 9: The dependency graph and cycles

**Files:**
- Create: `crates/loom-plan/src/graph.rs`, `crates/loom-plan/src/check/cycles.rs`
- Modify: `crates/loom-plan/src/check/{mod.rs, context.rs}` (add `graph`, register the rule)
- Create: `invalid/cycle-after/`, `invalid/cycle-needs/`, `invalid/cycle-through-epic/`
- Test: `crates/loom-plan/tests/check_cycles.rs`

**Interfaces:**
- Produces: `pub struct DependencyGraph` over `pub struct NodeId(String)` (the item's key, or its
  repo-relative path when it has none; a `Vec<NodeInfo>` beside it says epic or story and its
  path) with the four edge kinds of spec §5.5, built on `petgraph::graphmap::DiGraphMap` over
  node indices;
  `fn cycles(&self) -> Vec<Vec<NodeId>>` via `petgraph::algo::tarjan_scc` (components of size > 1,
  and self-loops). Items without a key are nodes by their path. Unresolvable references add no
  edge (Task 8 reports them). `cycles::check` gives one `cycle` finding per cycle, naming every
  member.
- [ ] **Step 1: Write the failing tests:** `// AC: TU-01.2` `cycle_through_after_is_reported`,
  `cycle_through_needs_across_components_is_reported`,
  `cycle_through_an_epics_prerequisites_is_reported` (epic A `after` epic B, a story of B `after`
  a story of A); `dropped_prerequisite_still_blocks_but_is_no_cycle` (R-r).
- [ ] **Step 2:** → FAIL. **Step 3:** Implement. **Step 4:** → PASS.
- [ ] **Step 5: Commit** `feat(plan): find dependency cycles`.

### Task 10: Unfinished work: ready epics and stories

**Files:**
- Create: `crates/loom-plan/src/check/{readiness.rs, epic_incomplete.rs, story_without_acs.rs, missing_key.rs}`
- Modify: `crates/loom-plan/src/check/mod.rs` (register)
- Create: the `invalid/` trees of this task
- Test: `crates/loom-plan/tests/check_completeness.rs`

**Interfaces:**
- Produces: `readiness::is_ready(ctx, item) -> bool`: `loom: ready` on the item or its epic, and
  not dropped (R-r); for stories outside an epic, their own mark (R-q). Each rule gives `Error`
  when the item is ready, `Note` otherwise (D3). `epic-incomplete` names which of goal, scope,
  requirement lines is missing.
- [ ] **Step 1: Write the failing tests:** `// AC: TU-01.2`
  `ready_epic_without_goal_is_an_error`, `ready_epic_without_scope_is_an_error`,
  `ready_epic_without_requirement_lines_is_an_error`,
  `story_without_acs_in_a_ready_epic_is_an_error` (a file story),
  `entry_only_story_in_a_ready_epic_is_an_error`,
  `ready_backlog_bug_without_criteria_is_an_error` (R-q),
  `epic_without_stories_is_valid` (a ready, complete epic with an empty `tickets/` → no finding),
  `unfinished_work_outside_ready_epics_gives_notes_only` (draft epic without requirements, entry
  stories → only `Severity::Note`), `ready_story_without_key_is_an_error`.
- [ ] **Step 2:** → FAIL. **Step 3:** Implement. **Step 4:** → PASS.
- [ ] **Step 5: Commit** `feat(plan): report unfinished work in ready epics`.

### Task 11: Directives by layer and contradiction marks

**Files:**
- Create: `crates/loom-plan/src/directives.rs` (layering), `crates/loom-plan/src/check/directives.rs`
- Modify: `crates/loom-plan/src/check/mod.rs` (register)
- Create: `invalid/directive-contradiction/`, `invalid/directive-contradicts-unknown/`,
  `invalid/directive-mark-in-project/`, `invalid/directive-unnamed/`,
  `invalid/directive-duplicate-name/`
- Test: `crates/loom-plan/tests/directives.rs`

**Interfaces:**
- Produces: `pub enum Layer { Project, Component }`;
  `pub struct ResolvedDirective { pub layer: Layer, pub kind: DirectiveKind, pub name: Option<String>, pub text: String, pub path: String, pub line: u32, pub contradicts: Option<String> }`;
  `impl PlanningRepo { pub fn directives_for(&self, component: &str) -> Vec<ResolvedDirective> }`
  in the order project engineering, project design, component engineering, component design,
  file order within each. Rules: `directive-contradiction` (both names, both files),
  `broken-reference` for a mark naming no project directive, a mark in `project.md` (R-k),
  `unnamed-directive`, and `duplicate-id` for two directives with one name in a file.
- [ ] **Step 1: Write the failing tests:** `// AC: TU-01.3`
  `directives_resolve_project_first_then_component` (names and layers in order on `valid/`),
  `marked_contradiction_is_reported_with_both_directives`,
  `mark_naming_no_project_directive_is_a_broken_reference`,
  `contradicted_directive_is_still_returned_flagged`; `mark_in_project_file_is_an_error`,
  `unnamed_directive_is_an_error`, `duplicate_directive_name_is_an_error`.
- [ ] **Step 2:** → FAIL. **Step 3:** Implement. **Step 4:** → PASS.
- [ ] **Step 5: Commit** `feat(plan): directives by layer and contradiction marks`.

### Task 12: JSON schemas

**Files:**
- Create: `crates/loom-plan/src/schema.rs`, `crates/loom-plan/schemas/*.schema.json` (generated
  by Step 4, committed)
- Test: `crates/loom-plan/tests/schemas.rs`

**Interfaces:**
- Produces: `pub fn schemas() -> Vec<(&'static str, schemars::Schema)>` with the names
  `initiative`, `epic`, `leaf`, `plan`, `document-front-matter`, `decision` (front-matter types)
  and `loom-toml`, `component-toml`, `tickets-toml`, `state-mirror`;
  `pub fn write_schemas(dir: &Path) -> std::io::Result<Vec<PathBuf>>` writing
  `<name>.schema.json`, pretty-printed (`serde_json::to_string_pretty`) with a trailing newline.
  Every `deny_unknown_fields` struct gives `additionalProperties: false`.
- [ ] **Step 1: Write the failing tests:** `// AC: TU-01.4`
  `committed_schemas_equal_generated_ones` (generate into `$CARGO_TARGET_TMPDIR/schemas` with
  `write_schemas`, compare byte for byte with `schemas/`; on mismatch the message names that folder
  and says to copy it over, or to run `cargo run -p loom -- plan schemas crates/loom-plan/schemas`
  once Task 13 exists),
  `every_front_matter_type_has_a_schema` (the six front-matter names are present),
  `po_templates_validate_against_their_schemas` (each `po-templates/` file's front matter, read
  as YAML into `serde_json::Value`, validates with `jsonschema::validator_for`; TOML templates
  likewise), `unknown_key_fails_schema_validation`.
- [ ] **Step 2:** → FAIL. **Step 3:** Implement `schema.rs`.
- [ ] **Step 4:** Run the test once; copy `$CARGO_TARGET_TMPDIR/schemas/*` (the path is in its
  failure message) into `crates/loom-plan/schemas/` and read one file to see it is the expected
  shape.
- [ ] **Step 5:** → PASS.
- [ ] **Step 6: Commit** `feat(plan): JSON schemas for every front-matter type`.

### Task 13: The `loom plan` commands

**Files:**
- Modify: `crates/loom/Cargo.toml` (`loom-plan`, `clap`, `anyhow`, `tracing`, `tracing-subscriber`),
  `crates/loom/src/main.rs`, `crates/loom/README.md`
- Create: `crates/loom/src/cli.rs` (clap types), `crates/loom/src/plan_cmd.rs`, `crates/loom/src/report.rs`
- Test: `crates/loom/tests/plan_check.rs` (the existing `tests/smoke.rs` stays as it is)

**Interfaces:**
- Consumes: `loom_plan::{read, check, schema::write_schemas, Finding, Severity}`.
- Produces: `loom plan check <path>` and `loom plan schemas <dir>`; `main` returns
  `std::process::ExitCode`: check → 0 without errors, 1 with an error, 2 on `ReadError` (R-l);
  `loom` with no subcommand prints help to stdout and exits 0 (the smoke test). `report::render(findings: &[Finding], out: &mut impl Write) -> io::Result<()>`:
  errors, then notes, grouped by path, each line `error[<rule>] <path>:<line>: <message>`, a
  final count line `N errors, M notes`.
- [ ] **Step 1: Write the failing tests** (through `env!("CARGO_BIN_EXE_loom")` and
  `std::process::Command`, fixtures from `../loom-plan/tests/fixtures/`):
  `// AC: TU-01.2` `check_exits_0_on_the_valid_tree`,
  `check_exits_1_on_every_invalid_tree` (iterates `invalid/*`, asserts exit 1 and that stdout
  names the tree's rule), `check_exits_2_on_a_path_that_is_not_a_directory`;
  `// AC: TU-01.4` `schemas_command_writes_every_schema`; `report_lists_errors_before_notes` (unit,
  in `report.rs`).
- [ ] **Step 2:** `cargo test -p loom` → FAIL. **Step 3:** Implement. **Step 4:** → PASS, and
  `tests/smoke.rs` still passes.
- [ ] **Step 5: Commit** `feat(cli): loom plan check and loom plan schemas`.

### Task 14: Docs, the real planning repo, the gate

**Files:**
- Modify: `crates/loom-plan/README.md` (what it reads, the rules, how to regenerate the schemas),
  `crates/loom-core/README.md` (the state enums)
- Modify: `docs/trial/P01-planning-repo-model.md` (step 2 numbers)
- Outside: `FPGSchiba/theos-planning`, branch `chore/loom-format-1`

- [ ] **Step 1:** Update the two READMEs. Commit `docs(plan): document the planning reader`.
- [ ] **Step 2:** Clone `FPGSchiba/theos-planning` into a scratch folder (`gh repo clone`), branch
  `chore/loom-format-1`, make exactly the changes of spec §5.10 (`format = 1` in `loom.toml` and
  each `component.toml`; `loom.toml` loses `[[component]]`; delete `components/*/epics/README.md`
  and `components/*/initiative/README.md`; the README's layout table follows umbrella §2.1).
  Touch nothing else.
- [ ] **Step 3:** Run `cargo run -p loom -- plan check <clone>` and read the output. Expected:
  exit 0. Any finding in Jann's own text is listed for him, not fixed.
- [ ] **Step 4:** Commit there (`chore: move the settings to Loom's format 1`), push, open the PR
  with the `check` output in its description. Never merge it (T11).
- [ ] **Step 5:** Run the full local gate: `cargo xtask gate`. Read the table. Every part passes
  (`ac-trace` lists TU-01.1 to TU-01.4 with tests). Fix and rerun until it does; never weaken a
  check.
- [ ] **Step 6:** Push the branch and open the PR `feat(plan): read and check a planning repo (P01)`
  with the template filled: every AC with its tests, every `Ruling:` from Tasks 3, 5 and any plan
  gap, the gate table, the theos-planning PR link. Don't enable auto-merge (the review step does
  that).
- [ ] **Step 7:** Fill step 2 of the trial record (tokens, cost, wall time, plan gaps raised,
  plan changes allowed) and commit `docs(trial): P01 implementation numbers`.
