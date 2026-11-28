# 0014 · Container tests run Podman on Linux and may pull pinned images

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P03 refinement

## Decision

Tests that need containers run rootless Podman and are built on Linux only. CI runs them on
every pull request; the local gate on Windows leaves them out, because Loom runs on Linux only
(decision 0004). They may pull container images, each named by its digest, so an image never
changes under a test. `CONTRIBUTING.md` §4 says so. Tests still need no other network.

## Options considered

- CI pulls the images in a step before the tests, and the tests stay offline: a change to the
  workflow, and a manual pull before running the tests in WSL.
- The container tests are marked `#[ignore = "needs podman"]`: CI would never prove the
  container criteria of P03, against T12.

## Consequences

The first pull of each image costs a few seconds; Podman caches it afterwards. A registry outage
fails the container tests until it is over.
