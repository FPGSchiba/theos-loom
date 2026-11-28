# Contributing to theos-loom

The same rules apply to everyone who changes this repo: Claude in its work sessions (see also
`CLAUDE.md`) and Jann. What a change must pass is in `docs/gate.md`. This file is protected:
changes wait for Jann.

## 1. Branches

`main` is protected: no direct pushes, no force pushes, squash merges only.

| Branch | Use |
|---|---|
| `spec/PNN-<slug>` | The phase spec of phase PNN |
| `phase/PNN-<slug>` | The implementation of phase PNN |
| `gate/<slug>` | Changes to the gate (CI, xtask, thresholds, lints) |
| `docs/<slug>` | Documentation outside a phase |
| `fix/<slug>` | A fix outside a phase |

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

| Type | For |
|---|---|
| `feat` | New behaviour for a user of Loom (cockpit, CLI, API) |
| `fix` | A bug fix |
| `perf` | Faster or leaner, same behaviour |
| `refactor` | Restructuring without changing behaviour |
| `test` | Tests only |
| `docs` | Documentation only |
| `build` | Cargo, pnpm, toolchain, dependencies |
| `ci` | GitHub workflows and the gate |
| `chore` | Anything else |
| `revert` | Reverts an earlier commit |

**Scopes:** `core`, `plan`, `store`, `run`, `gate`, `context`, `api`, `cli`, `web`, `xtask`,
`spec`, `docs`, `ci`. Leave the scope out only when a change really spans the whole repo.

**Summary:** imperative mood, lower case, no full stop, at most 72 characters:
`feat(plan): read the phases of a planning repo`.

**Breaking changes** get a `!` after the scope and a `BREAKING CHANGE:` footer. These are always
breaking: removed or renamed CLI flags or MCP tools, changed MCP tool schemas, a database
migration that cannot be rolled forward without loss, changed API routes, renamed settings keys.

**Footers**

| Footer | When |
|---|---|
| `Refs: TU-01.1, TU-01.3` | The acceptance criteria this commit works on |
| `Phase: P02` | Commits on a phase branch |
| `BREAKING CHANGE: …` | See above |
| `Co-Authored-By: Claude …` and `Claude-Session: …` | Every commit written by Claude |

Each commit on a branch should build and pass the tests on its own, so `git bisect` works on
branch history. On `main`, the squashed pull request is the unit.

## 3. Pull requests

- **Title** = the squash commit on `main` = a Conventional Commit that ends with the phase:
  `feat(store): runs and steps in PostgreSQL (P02)`. Spec PRs: `docs(spec): runs and steps (P02)`.
  Gate and docs PRs end with `(gate)` or `(docs)` instead of a phase.
- **Description:** the template in `.github/pull_request_template.md`, every section filled.
- **Size:** one phase per pull request. Gate, spec and documentation changes go in their own
  pull requests.
- **Merging:** pull requests that touch no protected path merge automatically when every check
  is green. Pull requests that touch a protected path (`docs/gate.md` §5) wait until Jann adds the
  label `approved`.
- **Keeping up:** update a branch from `main` only when it is ready to merge and next in line,
  not after every merge to `main`. Push when a task or a review round is finished, not after
  every commit.

## 4. Tests

- Test first (superpowers:test-driven-development). A test that proves an acceptance criterion
  carries `// AC: <id>` (`docs/gate.md` §4).
- Every test runs in the `test` check on Linux for every pull request. A test that takes minutes
  gets a name segment that starts with `slow_` (`store::slow_migrations_apply`), so that a later split
  can tell it apart; Loom has no such test yet.
- Tests that need PostgreSQL (from P03) read the URL of a throwaway database from
  `LOOM_TEST_DATABASE_URL`, never the real database.
- Tests that need containers (from P03) run Podman, rootless, and are built on Linux only: Loom
  runs on Linux only, so the local gate on Windows leaves them out and CI runs them. They may pull
  container images, each named by its digest. Tests need no other network.
- A test that needs an external service is marked `#[ignore = "needs <service>"]` and runs on
  request.
- Fixtures live in `crates/<crate>/tests/fixtures/`; synthetic fixtures say so in their file.

## 5. Documentation

Documentation under `docs/` (except `docs/plans/` and `docs/trial/`), this file, `CLAUDE.md` and
`.claude/` are protected. Decisions are recorded in `docs/decisions/`. Acceptance criteria change
only in `docs/specs/00-umbrella.md`.

## 6. Language and files

English everywhere. README files are named README. UTF-8 and LF for every text file we write.
