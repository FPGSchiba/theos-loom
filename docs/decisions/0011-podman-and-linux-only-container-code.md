# 0011 · Podman only; the container code is built on Linux only

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P03 refinement

## Decision

Runs use rootless Podman, driven through its CLI. The container code of `loom-run` and its tests
are built only on Linux (`cfg(target_os = "linux")`). CI proves them on every pull request; the
local gate on Windows builds and tests everything else (stream parsing, caps, prices, clones, the
store) and leaves the container layer out. Nothing new is installed on Windows; running the
container tests locally means the gate in WSL with Podman there.

## Options considered

- Podman or Docker behind one setting: Docker Desktop on Windows isn't rootless, so the rootless
  criterion couldn't be proven there anyway.
- Podman required on Windows too (`podman machine`): one more thing to install and keep, for a
  target Loom never runs on (decision 0004).

## Consequences

Loom's VM needs Podman (P19). The container tests' image rules are in decision 0014.
