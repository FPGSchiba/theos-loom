# 0004 · The local gate runs on Windows and Linux

Date: 27 Nov 2026 · Decided by: Jann · Surfaced by: P00 refinement

## Decision

`cargo xtask gate` is supported on Windows and on Linux. CI runs on Linux only. The port keeps
theos-editor's Windows handling in `xtask` (paths with `\`, names that differ only in case,
`cfg(windows)`), and `gate/cfg-names.toml` keeps `windows`. Loom itself (the server, the runs,
the containers) targets Linux only; "Linux only" in the P00 spec is about Loom, not about its
development tools.

## Options considered

- The local gate only in WSL, with the Windows handling removed: every session of the trial
  runs on Jann's Windows machine, so each one would call into WSL on a worktree under `/mnt/d`,
  with Rust, Node, pnpm, gitleaks, clang and mold installed there too.
- No supported local gate until Loom runs in its Linux VM: TU-00.3 could not be proven during the
  trial.

## Consequences

The Windows paths of `xtask` are tested only by local runs, never by CI; a Windows-only bug shows
up in a local run. The pinned tools (`gate/tools.toml`) must install on Windows as well; pnpm comes
through Corepack and gitleaks through `winget`.
