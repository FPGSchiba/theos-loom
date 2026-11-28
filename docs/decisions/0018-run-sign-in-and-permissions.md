# 0018 · How a run signs in, and what it may do

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P03 refinement

## Decision

- **Sign-in:** a long-lived token for Jann's Claude plan, made once with `claude setup-token`,
  passed into each run's container as an environment variable from Loom's secret store. No
  credentials file is copied into a container.
- **Permissions:** runs use `--dangerously-skip-permissions`. The container is the sandbox (its own
  clone, rootless, no home LAN, nothing else mounted). Agents still ask: their questions go to the
  decider (`ask_decider`), which is not a permission prompt.

## Options considered

- Copying `~/.claude`'s credentials file into each container: its refresh token rotates, so
  parallel runs would sign each other out, and every container would hold the full sign-in state.
- An allowlist of commands instead of skipping permissions: agents run very varied shell
  commands, so runs would keep stopping on a denied one with nobody to answer.

## Consequences

The token can be revoked on its own. Its value never appears in a config, an event or a log.
