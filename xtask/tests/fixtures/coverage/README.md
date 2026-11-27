# Coverage fixture

`llvm-cov.json` is **synthetic**. It has the shape of the export that
`cargo llvm-cov nextest --json` writes (`type` `llvm.coverage.json.export`, version 3.1.0), cut
down to what `parses_llvm_cov_fixture` needs: per file, the `filename` and `summary.lines`. It was
written by hand and is not the output of a real run, so its numbers mean nothing.

Its paths are for a repository at `/work/repo`. The files are chosen to cover each rule of the
parser:

- two files of `theos-content`, one in a nested folder, whose lines are summed;
- `theos-mcp` with Windows separators (`\work\repo\...`), which count like `/`;
- `theos-vcs` with a file that has no instrumented lines;
- `xtask` with two files under `xtask/src/`;
- files that must not count: `tests/` of a crate, a fixture below `xtask/tests/`, `build.rs`
  next to `src/`, a crate of another repository whose path starts like the root
  (`/work/repo-other`), and a dependency in the cargo registry.
