# 0001 · GitHub-hosted runners, and the repos go public

Date: 27 Nov 2026 · Decided by: Jann · Surfaced by: P00 refinement

## Decision

CI runs on GitHub's standard hosted runners. theos-loom, theos-editor and Theos-Engine are made
public, so those runners cost nothing. theos-planning and tower-of-theos stay private, because
they hold game canon.

## Options considered

- A self-hosted runner on the home server (no minute limit, but one more VM to look after, and
  unsafe for public repos).
- Hosted runners on private repos with a minimal gate (the editor ran out of included minutes
  this way, see its P50).

## Consequences

The gate can stay complete on every pull request. It's still kept lean and cached (P00 §5.7),
for speed. No secret may ever enter these repos.
