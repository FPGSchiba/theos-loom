# P01 · Planning repo model — phase spec

Status: approved by Jann (decider approval, trial step 1) · 28 Nov 2026
Source: `docs/specs/00-umbrella.md` §9 P01, with §2.1 as of umbrella version 9 (PR #11).
Scope and acceptance criteria come from there; this spec decides how.

## 1. Goal

After P01, Loom can read a project's planning repo and say what is wrong with it:

- `loom-plan` reads every file of the planning repo (umbrella §2.1) into typed values, the one
  place in Loom where planning files are parsed (directive 7 of the project: validate at the
  boundary, trust inside);
- `loom plan check <path>` reports mistakes and unfinished ready work, and exits non-zero on any
  error;
- directives come out layered, project first, then component;
- a JSON schema for every front-matter type is generated from the models and committed, so the
  BMad fork's templates can validate against it.

The planning repo is the **BMad PO's ticket tree** (decision 0020): Jann plans with BMad's PO
agent (Sofia) and her tools must keep working on it. Loom reads her tree as it is and adds a few
keys of its own. Later phases build on these models: P02 (state and `_state/`), P05 (picking up
ready work), P10 (indexing), P11 (briefings), P20 (the board).

## 2. Acceptance criteria (verbatim from the umbrella)

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

## 3. Scope

In scope: the models and reader in `loom-plan`; the state names of §3.2 as plain enums in
`loom-core`; the subcommands `loom plan check` and `loom plan schemas` in the `loom` binary;
the committed schemas; fixtures; a PR to `theos-planning` that brings its placeholders to the new
format (§5.10).

Out of scope: writing anything to a planning repo (P02 writes `_state/`, P20 the board moves);
state transitions (P02); polling git (P05); the index (P10); changing the PO's templates to carry
Loom's keys (P04, the workflow kit); the `_bmad/` config (Jann sets it up with the BMad installer).

## 4. Decisions

### 4.1 Decided by Jann (refinement, 28 Nov 2026)

| # | Decision | Record |
|---|---|---|
| D1 | The planning repo is the BMad PO's ticket tree, with her layout, `tickets.toml` files and templates. Umbrella §2.1 says so from version 9. | 0020 |
| D2 | `loom: ready` / `loom: paused` start and pause work; no key is draft; dropped is the PO's `status: dropped`; priority is the build order in `tickets.toml`. | 0021 |
| D3 | `check`: mistakes are errors anywhere; unfinished work is an error only in a ready epic (or a ready story); elsewhere a note. An epic with no stories is valid. | 0022 |
| D4 | Directives are named by their bold name; `Contradicts: <name>` marks a contradiction, which is an error. | 0023 |
| D5 | Every epic and story has a project-unique `key`; criterion ids are key plus number; `needs` holds cross-component prerequisites by key. | 0024 |
| D6 | TU-20.3 reworded: a board move commits the one field it changes. Not built here. | 0025 |

### 4.2 Claude's calls (Rulings, for Jann to check)

- **R-a Fixtures are a made-up project**, not copies of `theos-planning`. — Loom is
  project-neutral, and made-up files can't drift from the real ones. — Cost if wrong: a format
  detail of the real repo is missed; the PR in §5.10 catches it.
- **R-b The format source is the PO agent of the `requirements-bmad` plugin, version
  6.13.0-next** (`skills/method/bmad-agent-po/assets/*-template.md`,
  `assets/tickets-template.toml`, `scripts/tickets.py`, `tools/ticket-tree-rules.md`). The key
  lists in §5.2 are taken from there. — It is what Jann plans with. Reconciling it with the fork
  of umbrella §4 is P04's work. — Cost if wrong: key lists change when the fork does; each model
  is one file.
- **R-c Components are the `components/*/component.toml` folders**; `loom.toml` has no component
  list. — One source of truth (project directive 9). — Cost: none known; the current placeholder
  `loom.toml` loses its `[[component]]` tables.
- **R-d Every Loom settings file carries `format = 1`** (`loom.toml`, `component.toml`, `_state`
  files). — Project directive 11 (versioned formats). — Cost: one line per file.
- **R-e Free-form files are `Document`s**: path, optional raw front matter, body. Their front
  matter isn't validated. That covers `README.md`, initiative companions (brief, PRD, UX,
  architecture), `.memlog.md`, `canon/`, `archive/`, and the body of plan files. — Their shape is
  not Loom's; validating them would break on every BMad or import change. TU-01.1's "unknown
  front-matter keys" applies to the typed front-matter types of §5.2. — Cost if wrong: a typo in a
  free-form file's front matter goes unnoticed.
- **R-f `_bmad/`, `.git/` and dotfiles other than `.memlog.md` are skipped.** — `_bmad/` is the
  kit's own config.
- **R-g Unknown keys are errors in `tickets.toml` and the TOML settings too**, not only in front
  matter (the PO's `tickets.py` ignores unknown keys in `tickets.toml`; Loom does not). — A
  misspelt `after` or `loom` there would silently change the plan. — Cost: a new PO key breaks
  `check` until the model gains it.
- **R-h `_state/` files get a typed model in P01**; P02 writes them. The state names of umbrella
  §3.2 become plain enums in `loom-core` (no transitions; P02 adds those). — "Every file of §2.1"
  includes `_state/`, and T1 puts domain types in `loom-core`.
- **R-i Planning-repo decision records** carry front matter `kind` (`decision` or `answer`) and
  `date` (`YYYY-MM-DD`). File names are `NNNN-<slug>.md`, and numbers are unique. — P12 writes
  `answer` records; Jann's own are `decision`. The body is free.
- **R-j A component has at most one initiative folder** (`initiative-<slug>/`), which may be a
  parent initiative whose children are folders beside it. Two top-level initiatives in one
  component are an error. A component with no initiative yet is valid. — Umbrella §2: one parent
  initiative per component.
- **R-k A `Contradicts:` line in `directives/project.md`** is an error (only component directives
  may carry one). An ordered list item in a directives file without a bold name is an error (it
  can't be referred to).
- **R-l `check` prints a plain-text report**; there is no JSON output yet. Findings are
  `serde::Serialize`, so P13 can serve them as they are. Exit codes: 0 without errors (notes
  allowed), 1 with any error, 2 when the path can't be read as a planning repo.
- **R-m Schemas live in `crates/loom-plan/schemas/`**, one `<type>.schema.json` per type, written
  by `loom plan schemas <dir>`. A test fails when the committed files differ from the generated
  ones. — Committed files are what the BMad fork fetches; the test keeps them from drifting.
- **R-n `layout` covers ticket-shaped paths only**: a leaf file, plan file, `tickets.toml` or
  `epic-*` folder where the PO's tree doesn't allow it (the PO's own "earlier layout" refusal),
  two top-level initiatives in a component, a `component.toml` whose `id` isn't its folder. Any
  other file or folder is a `Document`. — The planning repo also holds Jann's free notes;
  flagging every unknown folder would make `check` red for no mistake. — Cost if wrong: a
  misplaced non-ticket file goes unnoticed.
- **R-o The file is truth once it exists** (the PO's rule). When a story has both a pulled file
  and a `tickets.toml` entry, the file's `key`, `loom` and `needs` count. If the entry carries one
  of them too with another value, that is the error `drift`; the same value twice is fine, never
  a `duplicate-id`. — It mirrors how `tickets.py` treats `after` and `hitl`. — Cost: none known.
- **R-p `_state/<component>/<epic key>.toml`**: the `<epic>` of umbrella §3.2 is the epic's `key`
  (unique in the project; slugs are not, across child initiatives). Stories outside an epic go in
  `_state/<component>/backlog.toml`. `[stories]` is keyed by story key. A mirror exists only for
  keyed items, which every ready item is. — P02 writes these files; P01 fixes their model and
  schema. — Cost if wrong: a format change in P02 before any file exists.
- **R-q Stories outside an epic** (`backlog/` leaves, and `[[entry]]` stories of an initiative
  without epics) may carry `loom: ready` and are then checked like a ready story: criteria and
  `key` required. In the dependency graph they have no epic edges. Whether and how Loom runs them
  without an epic intake is decided in P05 and P11. — The PO's tree has them, so the model must.
  — Cost if wrong: P05 refuses them, and `check` should have said so; one rule changes.
- **R-r A dropped item still blocks** what waits for it (the PO's rule) and is exempt from the
  ready rules.

## 5. Design

### 5.1 The tree

```text
<planning repo>/
  loom.toml                         LoomToml
  vision.md                         Vision            (Document + optional `status: draft`)
  directives/project.md             DirectiveFile     (layer project, engineering and design)
  components/<id>/
    component.toml                  ComponentToml     (`id` equals the folder name)
    directives/engineering.md       DirectiveFile     (layer component, engineering)
    directives/design.md            DirectiveFile     (layer component, design)
    initiative-<slug>/              Initiative
      initiative-<slug>.md          InitiativeFile
      <companions>                  Document
      tickets/tickets.toml          Breakdown         ([[epic]], [[entry]] or [[initiative]])
      tickets/epic-<slug>/          Epic
        epic-<slug>.md              EpicFile
        tickets/tickets.toml        Breakdown         ([[entry]])
        tickets/<type>-<slug>.md    LeafFile          (story, spike or bug)
        tickets/<type>-<slug>-plan.md PlanFile
    backlog/<type>-<slug>.md        LeafFile          (and its plan)
  canon/**, archive/**              Document
  decisions/NNNN-<slug>.md          DecisionRecord
  _state/<component>/<epic key>.toml StateMirror      (and backlog.toml, R-p)
  README.md, any other file         Document
```

Names follow the PO's rules (`tickets.py`): a leaf file matches `^(?:.+-)?(story|spike|bug)-(.+)\.md$`,
an epic folder `^(?!(?:story|spike|bug)-)(?:.+-)?epic-[^/]+$` (a tracker id may prefix both). A
leaf file joins its `tickets.toml` entry through its front-matter `id`; a plan joins its leaf
through `ticket`. A leaf file without an entry is allowed (the PO's "leaf with no entry").

The reader returns one `PlanningRepo` value: the project settings, the components with their
initiative trees, the directives, decision records, state mirrors and documents, plus the
**findings** it met while reading (a file that fails to parse is a finding, never an abort, so
one bad file doesn't hide the others).

### 5.2 Typed models and their keys

Every typed model is a `serde` struct with `#[serde(deny_unknown_fields)]` and a `schemars`
derive. A key the model doesn't know is the finding `unknown-key`, naming the file and the key
(TU-01.1). PO keys are kept with their PO meaning; Loom keys are marked **(Loom)**.

| Model | Keys |
|---|---|
| `LoomToml` | `format`; `[project]` `id`, `name`; optional `[budget]` `weight`; optional `[notify]` `target` |
| `ComponentToml` | `format`; `[component]` `id`, `name`, `repo`, `default_branch`, optional `kit_version`, `forbidden_paths` (globs, checked with `globset`), `slicing` (`auto` or `review`, default `auto` as umbrella §3.1 says), optional `spec_path` and `plan_path` (defaults of umbrella §4) |
| `InitiativeFile` front matter | `tracker_id`, `type: initiative`, `title`, `parent`, `sources`, `companions`, `after`, `status` (`in-progress`, `done`, `dropped`), `risk`, `remote`, `tracker_status` |
| `EpicFile` front matter | `tracker_id`, `type: epic`, `title`, `parent`, `covers`, `sources`, `companions`, `after`, `status`, `refined`, `risk`, `remote`, `tracker_status`; **(Loom)** `key`, `loom`, `needs` |
| `LeafFile` front matter (story, spike, bug) | `tracker_id`, `id`, `type`, `title`, `parent`, `covers`, `sources`, `companions`, `after`, `assignee`, `refined`, `hitl`, `risk`, `estimate`, `severity` (bugs only), `status` (older files; read only when there is no plan), `remote`, `tracker_status`; **(Loom)** `key`, `loom`, `needs`, `origin` (`sliced`) |
| `PlanFile` front matter | `ticket`, `type` (`feature`, `bugfix`, `refactor`, `chore`), `status` (the PO's eight statuses), `assignee`, `blocked_at`, `blocked_reason`, `baseline_revision` |
| `Breakdown` (`tickets.toml`) | `next_id`; `[[epic]]` `id`, `slug`, `title`, `description`, `covers`, `after` (ids or `{ epic, needs }` tables); `[[entry]]` `id`, `type`, `title`, `description`, `verify`, `covers`, `after`, `hitl`, `risk`, `estimate`, `unknown`, `references`, `notes`, `refine`, `plan_checkpoint`, `done_checkpoint`, **(Loom)** `key`, `loom`, `needs`; `[[initiative]]` `id`, `slug`, `title`, `description`, `covers`, `after`. One kind of table per file. |
| `Vision`, `DirectiveFile` front matter | optional `status: draft` |
| `DecisionRecord` front matter | `kind`, `date` (R-i) |
| `StateMirror` | `format`; `epic` (an `EpicState`; absent in `backlog.toml`); `[stories]` story key → `StoryState` (R-p) |

The builder checks each list against the R-b sources before writing the model, and adds a key the
templates carry that this table missed, as a Ruling. Value types are checked where they enter:
`key` matches `^[A-Z][A-Z0-9]*-[0-9]+$`; `loom` is `ready` or `paused`; ids are the PO's
`^[0-9A-Za-z]+$` (TOML integers and strings are one id, as in `tickets.py`); `after` takes the
PO's forms (`3`, `"2.3"`, `"epic-<slug>"`); `needs` takes keys.

**Bodies** are read with `pulldown-cmark`, already in the workspace:

- *Epic*: sections by `##` heading. **Goal** = Description and Outcome non-empty; **scope** =
  Boundaries non-empty; **epic ACs** = the requirement lines under Requirements, each starting
  `R<n>` (list item or paragraph), with the id `<key>.R<n>`.
- *Story*: criteria are the items of the ordered list under `## Requirements`, numbered as
  written; id `<key>.<n>`. A `Verify:` line alone is no criterion.
- *Spike and bug*: the same, under `## Acceptance Criteria`.
- *Directives*: every top-level item of an ordered list is a directive. Its name is the text of
  the leading bold span, with a trailing full stop removed. A line `Contradicts: <name>` inside it
  is a contradiction mark. In `project.md`, the `## Engineering` and `## Design` headings set the
  kind; `###` headings group and don't matter.
- Front matter is found with `pulldown-cmark`'s YAML metadata-block option (no hand-written
  splitter), then deserialized with `serde-saphyr` (maintained, panic-free, serde-based; the
  older `serde_yaml` is archived and `serde_yml` is deprecated). TOML goes through the `toml`
  crate already in the workspace.

### 5.3 Crate layout (`crates/loom-plan/src/`)

One job per module (directive 1):

- `lib.rs`: the public API: `read(path) -> Result<PlanningRepo, ReadError>`, re-exports.
- `model/`: one file per model of §5.2 (`loom_toml.rs`, `component_toml.rs`, `initiative.rs`,
  `epic.rs`, `leaf.rs`, `plan_file.rs`, `breakdown.rs`, `directive.rs`, `decision.rs`,
  `state_mirror.rs`, `document.rs`), plus `ids.rs` (`Key`, `TicketId`, `CriterionId`,
  `AfterRef`, the value types with their checks).
- `parse/`: `front_matter.rs` (find and deserialize), `toml_file.rs`, `body.rs` (sections, lists,
  criteria, requirement lines), `directives.rs`.
- `read/`: walking the tree and classifying each path (`walk.rs`, `classify.rs`), building the
  `PlanningRepo` (`assemble.rs`).
- `finding.rs`: `Finding { severity, rule, path, line, message }`, `Severity { Error, Note }`.
- `check/`: one module per rule (§5.4) and `mod.rs` with the registry, a slice of rule functions
  `fn(&PlanningRepo) -> Vec<Finding>`. A new rule is one file plus one line.
- `graph.rs`: the dependency graph (§5.5).
- `directives.rs`: layering (§5.6).
- `schema.rs`: the list of schema types and their generation (§5.7).

`loom-core` gains `state.rs` with `EpicState` (`intake`, `slicing`, `active`, `blocked`, `done`)
and `StoryState` (`queued`, `refining`, `refined`, `implementing`, `in-review`, `blocked`,
`merged`), serialized in kebab-case.

Errors: `ReadError` (`thiserror`) only for what stops reading as a whole (the path is no
directory, I/O failure on the root). Everything about file content is a `Finding`. Logging with
`tracing`, one span per file read.

### 5.4 The rules of `check`

| Rule | Severity | What |
|---|---|---|
| `parse` | error | a typed file that doesn't parse (TOML or YAML syntax, wrong value type, bad key or id form) |
| `unknown-key` | error | a key the model doesn't know, naming the file and the key (TU-01.1) |
| `layout` | error | a ticket-shaped path where the PO's tree doesn't allow it (R-n); two top-level initiatives in a component (R-j); `component.toml` `id` not its folder name; more than one kind of table in a `tickets.toml` |
| `drift` | error | a story's file and its `tickets.toml` entry disagree on `key`, `loom` or `needs` (R-o) |
| `broken-reference` | error | an `after`, `needs`, `covers`, `parent`, plan `ticket` or `_state` path naming an unknown component, initiative, epic, story, key or requirement line; a `Contradicts:` naming no project directive |
| `cycle` | error | a cycle in the dependency graph (§5.5), naming every member |
| `duplicate-id` | error | two entries with one id in a `tickets.toml`; two epics or stories with one `key` in the project; two criteria with one number in a file; two directives with one name in a file; two decision records with one number |
| `directive-contradiction` | error | a component directive with a `Contradicts:` mark, naming both directives and both files (TU-01.3); a mark in `project.md` (R-k) |
| `unnamed-directive` | error | a directive without a bold name (R-k) |
| `epic-incomplete` | error if the epic is ready, else note | no goal, no scope, or no requirement line, saying which |
| `story-without-acs` | error if the story or its epic is ready, else note | a story, spike or bug (file or entry only) without numbered criteria |
| `missing-key` | error if ready, else note | an epic or story without `key` (its criteria have no ids) |

"Ready" means `loom: ready` on the item or on its epic (D3). An epic without stories is never a
finding (TU-01.2). A finding names the path relative to the repo root, the line when known, and a
message in plain words that says what to change.

### 5.5 Dependencies and cycles

Nodes are epics and stories. Edges, "waits for":

- a story → each prerequisite in its `after` (sibling, `<epic id>.<entry id>`, or a whole epic)
  and its `needs`;
- an epic → each prerequisite in its file's `after` and its `tickets.toml` table's `after`, and
  its `needs`;
- an epic → each of its stories (an epic is done when its stories are);
- a story → each prerequisite of its epic (it can't start before its epic may).

The graph is built with `petgraph`; `tarjan_scc` finds the cycles (each component with more than
one node, or a self-loop). The same graph is P08's scheduling input later.

### 5.6 Directives by layer

`PlanningRepo::directives_for(component) -> Vec<ResolvedDirective>` returns project engineering,
project design, component engineering, then component design, in file order. Each carries its
layer, kind, name, text, file and line. A directive with a contradiction mark is still returned,
flagged, so P11's briefing can show it while `check` blocks it.

### 5.7 JSON schemas

`schemars` (1.x) derives a schema for every front-matter type: `InitiativeFile`, `EpicFile`,
`LeafFile`, `PlanFile`, `Vision`/`DirectiveFile` front matter and `DecisionRecord`. The TOML
types (`LoomToml`, `ComponentToml`, `Breakdown`, `StateMirror`) get schemas too, because editors
can use them. `deny_unknown_fields` becomes `additionalProperties: false`. `loom plan schemas
<dir>` writes them (R-m).

### 5.8 The CLI

`loom` gets `clap` (workspace) subcommands `plan check <path>` and `plan schemas <dir>`, with
`anyhow` for errors (binary). The report lists errors first, then notes, grouped by file, and
ends with a count line. Output goes through a locked `stdout` writer (the workspace lints forbid
`println!`). The existing smoke test of `loom` with no arguments keeps passing: with no
subcommand, `loom` prints its help and exits 0.

### 5.9 Fixtures

`crates/loom-plan/tests/fixtures/`:

- `valid/`: a made-up project (R-a) with two components, one with a parent initiative and a
  child, epics in every state (draft, ready, paused, dropped), stories as files and as entries
  only, a spike, a bug in `backlog/`, a plan file, a cross-component `needs`, directives on both
  layers, decision records, a `_state/` mirror and free-form documents. `check` reports no errors
  on it.
- `invalid/<rule>-<case>/`: one small tree per finding case of §5.4, each with exactly the one
  defect its test asserts.
- `po-templates/`: the PO's templates (R-b) filled in with sample values and Loom's keys, so the
  schema test validates real template shapes.

### 5.10 The real planning repo

In the implementation step, after `check` works: a PR to `FPGSchiba/theos-planning` (branch
`chore/loom-format-1`) that adds `format = 1`, removes `loom.toml`'s `[[component]]` tables (R-c),
removes the old-layout placeholders `components/*/epics/README.md` and
`components/*/initiative/README.md` (they describe the layout of umbrella version 8; each
component starts without an initiative, which is valid, R-j), updates the README's layout table,
and leaves every other file as Jann wrote it. `loom plan check` is run on that branch, and its
output goes into the trial record and the P01 PR. Jann merges it himself (T11). If `check` reports
something in Jann's own text (a directive without a bold name, say), the PR doesn't edit it; the
finding is listed for Jann instead.

## 6. How each AC is proven

| AC | Tests (behaviour tests in `crates/loom-plan/tests/`, unit tests beside the code) |
|---|---|
| TU-01.1 | reading `valid/` yields every file as its typed model, checked per file kind; one test per typed file kind that an unknown key gives `unknown-key` with the file and the key in the message; documents keep their path and text |
| TU-01.2 | one test per rule and case of §5.4 against its `invalid/` tree (broken references of each kind, a cycle through `after`, through `needs` and through an epic's prerequisites, stories without ACs in a ready epic, epics missing goal, scope or requirement lines, each duplicate); an epic without stories gives no finding; unfinished work outside ready epics gives notes only; `loom plan check` exits 1 on each `invalid/` tree, 0 on `valid/`, 2 on a path that isn't a directory (`assert_cmd`-style, through `CARGO_BIN_EXE_loom`) |
| TU-01.3 | `directives_for` returns project before component, in file order, with layer and kind; a `Contradicts:` mark gives `directive-contradiction` naming both directives; a mark naming no project directive gives `broken-reference` |
| TU-01.4 | the committed schemas equal freshly generated ones; every front-matter type has a schema file; each `po-templates/` file's front matter validates against its schema (`jsonschema` crate, dev-dependency); a front matter with an unknown key fails validation |

Every test carries `// AC: TU-01.n`.

## 7. Interfaces other phases use

- `loom_plan::read(path) -> Result<PlanningRepo, ReadError>`, and `check(&PlanningRepo) -> Vec<Finding>`.
- `PlanningRepo`: components, initiatives, epics, stories with keys, criteria with ids,
  readiness, build order, the dependency graph, `directives_for`, decision records, state mirrors.
- `loom_core::state::{EpicState, StoryState}` (P02 adds the transitions).
- The schemas in `crates/loom-plan/schemas/` (P04's fork validates its templates against them).

## 8. Risks

- **The PO's format moves.** The plugin is at `6.13.0-next`; a template change breaks reading.
  Mitigation: the format source is pinned (R-b), the models are one file each, and the
  `po-templates/` fixtures show a change at once.
- **`deny_unknown_fields` and flattened structs don't mix in serde.** Models are written flat, one
  struct per file kind, no `#[serde(flatten)]`.
- **`serde-saphyr`'s version.** Its 3.x is a release candidate; take the latest stable release
  and pin it, like every workspace dependency. If it can't report unknown keys with the key name,
  fall back to `serde_norway` and record it as a Ruling.
- **New dependencies** (`serde-saphyr`, `schemars`, `petgraph`, dev: `jsonschema`) must pass
  `cargo deny`; each is MIT or Apache-2.0 and maintained.

## 9. Left to the builder

The exact wording of messages and the report layout; the internal shape of `PlanningRepo`
beyond §7; how `walk` and `classify` split their work; the made-up project's content (no lore or
creative content, hard rule 7: a plain business app is enough).
