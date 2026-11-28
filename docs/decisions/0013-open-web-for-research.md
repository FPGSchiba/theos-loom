# 0013 · Runs reach the public web, for research; the home LAN stays blocked

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P03 refinement

## Decision

A run's container may reach any public HTTP(S) destination, through a proxy that records every
destination in the run's log. The home LAN, the database VM and every other private address range
stay blocked. Research tools (reading web pages, documentation services) are needed to hold the
quality directive; without them a worker can't look up how a library or tool really behaves.

T4 of the umbrella says so from version 8. TU-03.5 keeps its wording: "the destinations of T4"
are now public HTTP(S) destinations, and its test is a blocked request to a private address.

## Options considered

- **The strict list** (the rule until now): GitHub, the registries the repo declares and
  Anthropic. Web search still works, because it runs on Anthropic's side, but reading pages and
  documentation tools don't.
- **A curated list** of research sites per project in `loom.toml`: safer, but agents hit walls on
  every site nobody listed, and the list needs upkeep.
- An exception only for research agents is not possible: every agent of a run shares the
  container's network.

## Consequences

- A page with hidden instructions could get an agent to send a run's credentials somewhere. What
  limits that: the GitHub token is fine-grained (the project's repos, contents and pull requests
  only), the Claude sign-in is the only other credential in a container, and every destination is
  logged, so an unexpected one can be traced.
- The proxy filters by destination address after name resolution, so a public name that resolves
  to a private address is blocked too.
