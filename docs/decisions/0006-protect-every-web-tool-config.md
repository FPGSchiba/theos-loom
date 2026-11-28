# 0006 · Protect every config that can switch off `ui`

Date: 27 Nov 2026 · Decided by: Jann · Surfaced by: P00 final review, round 1

## Decision

Besides `web/eslint.config.*`, `web/tsconfig*.json` and `web/vite.config.*`, the protected paths
include `web/**/eslint.config.*`, `web/vitest.config.*` and `web/vitest.workspace.*`. The `lints`
part also refuses an `eslint.config.*` anywhere but at `web/`, the way it refuses a stray
`clippy.toml`.

## Options considered

- Protection only, without the `lints` refusal: a nested config would still change the verdict;
  it would only wait for approval.
- No change: ESLint 10 takes the config nearest to each linted file, and Vitest prefers
  `vitest.config.*` over `vite.config.*`, so either file silences `ui` without touching a
  protected path.

## Consequences

The `ui` part can only be loosened through a protected PR. A second ESLint config inside `web/`
fails the gate even when approved; one would need a change to `lints` first.
