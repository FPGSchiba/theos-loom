# Contributing to theos-editor

The same rules apply to everyone who changes this repo: Claude in its work sessions (see also
`CLAUDE.md`) and Jann. Releases are described in `docs/release.md`; what a change must pass is in
`docs/gate.md`. This file is protected: changes wait for Jann.

## 1. Branches

`main` is protected: no direct pushes, no force pushes, squash merges only.

| Branch | Use |
|---|---|
| `spec/PNN-<slug>` | The phase spec of phase PNN |
| `phase/PNN-<slug>` | The implementation of phase PNN |
| `gate/<slug>` | Changes to the gate (CI, xtask, thresholds, lints) |
| `docs/<slug>` | Documentation outside a phase |
| `fix/<slug>` | A fix outside a phase |
| `release-plz-*` | Created by the release pipeline only |

One branch, one pull request. Delete the branch after merging (GitHub does this automatically).

## 2. Commits

Every commit follows [Conventional Commits 1.0](https://www.conventionalcommits.org/en/v1.0.0/).
The gate checks every commit of a pull request (`commit-msg`) and its title (`pr-title`).

```text
<type>(<scope>)<!>: <summary>

<body: why the change is needed, wrapped at 100 columns>

<footers>
```

**Types**

| Type | For | In the changelog |
|---|---|---|
| `feat` | New behaviour for a user of the editor, CLI or MCP | Features |
| `fix` | A bug fix | Fixes |
| `perf` | Faster or leaner, same behaviour | Performance |
| `refactor` | Restructuring without changing behaviour | – |
| `test` | Tests only | – |
| `docs` | Documentation only | Documentation |
| `build` | Cargo, pnpm, toolchain, dependencies | – |
| `ci` | GitHub workflows and the gate | – |
| `chore` | Anything else, including release commits | – |
| `revert` | Reverts an earlier commit | Reverts |

**Scopes:** `content`, `vcs`, `studio`, `cli`, `mcp`, `editor` (Tauri), `ui`, `generators`,
`xtask`, `gate`, `spec`, `docs`, `design`, `release`. Leave the scope out only when a change
really spans the whole repo.

**Summary:** imperative mood, lower case, no full stop, at most 72 characters:
`feat(content): parse note callouts with their states`.

**Breaking changes** get a `!` after the scope and a `BREAKING CHANGE:` footer. These are always
breaking: removed or renamed CLI flags or MCP tools, changed MCP tool schemas, a new
`schema_version` of the data model, a new generator protocol version, renamed settings keys.

**Footers**

| Footer | When |
|---|---|
| `Refs: CE-09.1, CE-09.3` | The acceptance criteria this commit works on |
| `Phase: P02` | Commits on a phase branch |
| `BREAKING CHANGE: …` | See above |
| `Co-Authored-By: Claude …` and `Claude-Session: …` | Every commit written by Claude |

Each commit on a branch should build and pass the tests on its own, so `git bisect` works on
branch history. On `main`, the squashed pull request is the unit.

## 3. Pull requests

- **Title** = the squash commit on `main` = a Conventional Commit that ends with the phase:
  `feat(content): records and grammars (P02)`. Spec PRs: `docs(spec): records and grammars (P02)`.
  Gate and docs PRs end with `(gate)` or `(docs)` instead of a phase. The title becomes a line in
  the changelog, so write it for someone reading release notes.
- **Description:** the template in `.github/pull_request_template.md`, every section filled.
- **Size:** one phase per pull request. Gate, spec and documentation changes go in their own
  pull requests.
- **Merging:** pull requests that touch no protected path merge automatically when every check
  is green. Pull requests that touch a protected path (`docs/gate.md` §5) wait until Jann adds the
  label `approved`. Release pull requests are merged by Jann only.

## 4. Tests

- Test first (superpowers:test-driven-development). A test that proves an acceptance criterion
  carries `// AC: <id>` (`docs/gate.md` §4).
- Tests run on Linux for every pull request, and on Windows and macOS weekly, on demand (the
  labels `windows` and `macos`) and before every release; they need no network. Tests that need
  ComfyUI, an LLM endpoint or another external service are marked
  `#[ignore = "needs <service>"]` and run on request.
- Fixtures live in `crates/<crate>/tests/fixtures/`; synthetic fixtures say so in their file.

## 5. Versions and tags

- **One product version.** All crates share the version in `[workspace.package]` of the root
  `Cargo.toml`. The editor app, `theos` and `theos-mcp` report it; the Tauri app takes its version
  from `Cargo.toml`. `editor-ui/package.json` is private and its version is not used.
- **SemVer.** While the editor is built the version stays below 1.0: a minor bump for new
  features or breaking changes, a patch bump for fixes. Release candidates are `-rc.N`.
- **Milestones.** 0.1.0 when slice S2 is done (CLI and MCP read usable), 0.2.0 after S3, 0.3.0
  after S4, 0.4.0 after S5, 0.5.0 after S6, 0.6.0 after S7, 0.7.0 after S8, 1.0.0 when S9 is done
  and Jann accepts the editor, 1.1.0 after S10, 1.2.0 after S11. Releases in between whenever
  useful.
- **Tags** are `vMAJOR.MINOR.PATCH[-rc.N]`, annotated, on `main`, and created only by the release
  pipeline. Nobody creates, moves or deletes a tag by hand. A bad release is fixed by a new
  patch release, never by moving a tag.
- **Separate version numbers** that are not the product version: `schema_version` of the data
  model (`docs/specs/data-model.md`), `protocol` of the generator protocol (`docs/generators.md`),
  and the major of `format_version` of the map package (`docs/map-package.md`). Raising one of
  them is a breaking change of the product. The map generator also reports a version of its own
  in `describe` (`theos-maps@X.Y.Z`), raised whenever its outputs change (`docs/generators.md`
  §6); that is not a breaking change.

## 6. Documentation

Documentation under `docs/` (except `docs/plans/` and `docs/status.md`), this file, `CLAUDE.md`
and `design/` are protected. Decisions are recorded in `docs/decisions/`. Acceptance criteria
change only in `docs/specs/00-umbrella.md`.

## 7. Language and files

English everywhere. README files are named README. UTF-8 and LF for every text file we write.
