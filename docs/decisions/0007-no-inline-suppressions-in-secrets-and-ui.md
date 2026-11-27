# 0007 · No inline suppressions in `secrets` and `ui`

Date: 27 Nov 2026 · Decided by: Jann · Surfaced by: P00 final review, round 1

## Decision

The `secrets` part runs gitleaks with `--ignore-gitleaks-allow`, so a `gitleaks:allow` comment
doesn't hide a finding; the only exceptions live in the protected `.gitleaksignore`.
`web/eslint.config.js` sets `linterOptions: { noInlineConfig: true,
reportUnusedDisableDirectives: "error" }`, so no `eslint-disable` comment switches a rule off.

## Options considered

- Only the gitleaks flag: TypeScript could still switch off any rule inline, including the one
  that bans `@ts-ignore`; that is the TypeScript version of `#[allow]`.
- No change: both comments pass the gate without touching a protected path.
- A rule against `it.skip` / `describe.skip` without a reason: left for later, to keep P00's
  scope.

## Consequences

Stricter than Rust, where `#[expect]` with a reason is allowed: TypeScript code can't suppress a
lint rule inline at all. Every exception goes into the protected ESLint config and waits for
Jann.
