# 0002 · MIT licence

Date: 27 Nov 2026 · Decided by: Jann · Surfaced by: P00 refinement

## Decision

theos-loom, theos-editor and Theos-Engine are licensed under MIT, copyright Jann Erhardt.

## Options considered

- Apache-2.0 and MIT, dual-licensed (the Rust ecosystem's default).
- AGPL-3.0 (makes changes to a hosted Loom public, but keeps game developers away from an engine).

## Consequences

`deny.toml` allows only MIT-compatible licences in dependencies (P00 §5.6).
