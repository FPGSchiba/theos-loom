# 0017 · Raw run streams live in PostgreSQL, from P03

Date: 28 Nov 2026 · Decided by: Claude, by Jann's leave · Surfaced by: P03 refinement

## Decision

Every run's raw stream (T7) is written to PostgreSQL while the run goes, in chunks, and deleted
after 30 days. P03 builds it, as the store's first table, rather than leaving it to a follow-up in
P02: TU-03.2 is then proven against the real store. Jann did not want a file per run and left the
choice between P03 and P02 to Claude.

## Options considered

- A compressed file per run on Loom's disk: Jann's least preferred.
- PostgreSQL as a follow-up of P02, with a stand-in store in P03: TU-03.2 would be proven only
  against the stand-in.

## Consequences

P03 is the first phase whose tests need a throwaway PostgreSQL; locally that is one Docker
command, and the `cargo xtask` helper stays with P02. One place to back up (P19's nightly dump).
