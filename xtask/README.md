# xtask

Library and binary. The gate tooling of Loom (`docs/gate.md`): the parts of every required check,
the rule that skips the parts a docs-only change cannot affect, and the comparison of the GitHub
settings with the files in `gate/`. Run it as `cargo xtask`. Exit code 0 means a part passed, 1
that it found something, and 2 that it could not run.

## Subcommands

| Subcommand | What it does |
|---|---|
| `gate` | Runs `tools`, `fmt`, `lints`, `commit-msg`, `secrets`, `clippy`, `docs`, `deny`, `test`, `coverage`, `ui` and `ac-trace` and writes one table; `--only` and `--skip` choose parts |
| `pr` | Runs `pr-title` and `pr-body` and writes one table |
| `tools` | Checks the tool versions pinned in `gate/tools.toml` |
| `fmt`, `clippy`, `test`, `docs`, `deny` | One or two cargo commands each |
| `lints`, `commit-msg`, `secrets`, `coverage`, `ui`, `ac-trace` | The parts that are code of this crate |
| `pr-title`, `pr-body`, `approval` | The parts that need a pull request |
| `github check`, `github apply` | Compare the live repository with `gate/ruleset-main.json` and `gate/repo-settings.toml`, or make it like them (only after Jann agrees) |

The checks (required status checks) and the parts each runs are in `.loom/checks.toml`.

## Modules

- `verdict`, `report`: what a part returns, and how it is written (table, annotations, summary).
- `gate`, `need`, `changes`, `code_paths`: the gate's runner, which parts are needed, the changed
  paths, and `gate/code-paths.toml`.
- `checks`: one module per part; `checks::lints` and `checks::approval` have a module per rule.
- `github`: the settings as files and the `gh` command line behind them.
- `umbrella`, `branch`, `conventional`: the umbrella spec's phases and criteria, the branch names
  and the commit message rules that `CONTRIBUTING.md` describes.
- `git`, `files`, `process`, `markdown`, `view`: the only places that start git and other
  programs, walk the tree, read Markdown, and read the code cargo compiles.

Tests are in `tests/`; fixtures are in `tests/fixtures/`.
