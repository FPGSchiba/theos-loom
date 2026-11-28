# 0005 · The toolchain's default linker

Date: 27 Nov 2026 · Decided by: Jann · Surfaced by: P00 planning (approved with the plan)

## Decision

Loom links with the Rust toolchain's default linker. On `x86_64-unknown-linux-gnu` that is
`rust-lld` since Rust 1.90, so CI installs nothing for linking and `.cargo/config.toml` holds
only the `xtask` alias. This replaces "`mold` with `clang`" in the P00 spec (§5.1, §5.2).

## Options considered

- `mold` with `clang`, as the spec first said: one more install step in every Rust job, for
  little gain over `rust-lld`.

## Consequences

A few seconds of link time per job at most. `mold` can still come later in a `gate/` PR if a
measurement shows it pays.
