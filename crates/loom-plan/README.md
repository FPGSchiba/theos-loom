# loom-plan

Loom's library for the planning repo: it reads the repo into typed models, checks it, resolves
directives by layer and generates the JSON schemas of the front-matter types. The design is in
[`docs/specs/phases/P01-planning-repo-model.md`](../../docs/specs/phases/P01-planning-repo-model.md)
(the tree is §5.1). The command line is `loom plan check` and `loom plan schemas`, see
[`crates/loom/README.md`](../loom/README.md).

## What it reads

The tree of the BMad PO agent (initiatives, epics, stories, `tickets.toml`, plan files, `_state`
mirrors) plus Loom's own keys and files. A sketch:

```text
loom.toml                      project settings (format = 1)
vision.md  directives/project.md  decisions/NNNN-<slug>.md
components/<id>/
  component.toml
  directives/engineering.md, design.md
  <initiative-folder>/
    <initiative>.md
    tickets/
      tickets.toml
      <epic-folder>/<epic>.md, tickets/<stories, spikes, bugs, plans>, tickets.toml
  backlog/                     bugs and stories outside an epic
_state/<component>/<epic-key or backlog>.toml
```

Unknown front-matter and settings keys are errors that name the file and the key.

## API

- `read(root: &Path) -> Result<PlanningRepo, ReadError>`: reads the repo. A file that does not
  parse becomes a finding in `PlanningRepo::read_findings`; only a `root` that is not a directory
  is an error.
- `check(&PlanningRepo) -> Vec<Finding>`: the read findings plus every check rule, errors first,
  then by path and line. `check` fails (in the CLI) on any `Severity::Error`; notes never fail it.
- `PlanningRepo`: the read repo (components, initiatives, epics, stories, directive files,
  decisions, state mirrors).
- `PlanningRepo::directives_for(component) -> Vec<ResolvedDirective>`: project engineering,
  project design, component engineering, component design, each in file order, each with its
  `Layer`. A directive marked `Contradicts:` is still returned (with `contradicts` set).
- `DependencyGraph::of(&PlanningRepo)`: the graph of epics and stories (`nodes`, `waits_for`,
  `path_of`, `cycles`).
- `schema::schemas()` and `schema::write_schemas(dir)`: the schemas, see below.

## Check rules

A finding has a severity (`error` or `note`), a rule, a path, an optional line and a message.
`epic-incomplete`, `story-without-acs` and `missing-key` are errors when the item is ready
(`loom: ready`, itself or through its epic) and notes otherwise. The other rules are always errors.

| Rule | Reports |
| --- | --- |
| `parse` | A typed file that does not parse. |
| `unknown-key` | A key the model does not know. |
| `layout` | A path or file shape the tree does not allow. |
| `drift` | A story file and its `tickets.toml` entry disagree on `key`, `loom` or `needs`. |
| `broken-reference` | An `after`, `needs`, `covers`, `parent`, plan `ticket`, `_state` path or `Contradicts:` mark that names something that does not exist. |
| `cycle` | A cycle in the dependency graph, naming every member. |
| `duplicate-id` | Two things with one id: table rows, `key`s, criteria, decision numbers, directive names in a file. |
| `directive-contradiction` | A component directive marked as contradicting a project directive. |
| `unnamed-directive` | A directive without a bold name. |
| `epic-incomplete` | An epic without goal, scope or requirement line. An epic without stories is valid. |
| `story-without-acs` | A story, spike or bug without numbered criteria. |
| `missing-key` | An epic or story without a `key`. |

## Add a rule

1. Add a variant to `Rule`, its entry in `Rule::ALL` and its name in `Rule::name`
   (`src/finding.rs`).
2. Write one module in `src/check/` with `pub(crate) fn check(cx: &Context<'_>) -> Vec<Finding>`.
3. Declare the module in `src/check/mod.rs` and add one line, `<module>::check,`, to `RULES`.
4. Add `tests/fixtures/invalid/<rule>-<case>` and a test.

## Schemas

The schemas are generated from the models with `schemars` and committed in `schemas/`
(`<name>.schema.json`). After changing a model, regenerate them:

```sh
cargo run -p loom -- plan schemas crates/loom-plan/schemas
```

The test `committed_schemas_equal_generated_ones` fails when the committed files drift from the
models.

## Fixtures

In `tests/fixtures/`:

- `valid/`: a synthetic planning repo that reads and checks without errors.
- `invalid/<rule>-<case>/`: one small repo per finding; the tests expect that rule.
- `cases/`: repos for readiness, `after` names and other behaviour cases.
- `po-templates/`: the BMad PO templates, which must validate against the schemas.
- `models/`: single files for the model tests.
