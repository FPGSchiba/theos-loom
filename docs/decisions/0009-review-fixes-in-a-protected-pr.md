# 0009 · Review fixes inside a protected PR need no separate approval

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P00 verdict

## Decision

A PR that already touches protected paths waits for Jann's approval anyway. During its own final
review loop, its fix waves may change protected paths without asking Jann first. The PR lists
every protected path it changes, and Jann's approval of the PR covers them. Every other rule
stays: an implementation PR still never starts touching protected paths (hard rule 3), and no fix
ever weakens a check.

## Options considered

- Ask Jann before every fix wave that touches a protected path (the rule until now). In P00's
  review, four of seven questions to Jann were such approvals, and they took most of his time.

## Consequences

`CLAUDE.md`'s hard rule 3 and its list of reasons to stop and ask say so. Review fixes still go
through the full loop, so a fresh reviewer reads each of them.
