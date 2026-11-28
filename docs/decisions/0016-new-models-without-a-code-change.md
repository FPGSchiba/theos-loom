# 0016 · New models and CLI versions work without a code change

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P03 refinement

## Decision

New models and new versions of the Claude Code CLI keep working without a code change or a new
release of Loom (umbrella T14, from version 8):

- Model names and prices are data, loaded while Loom runs. The source is a public,
  machine-readable price list (LiteLLM's model price file), fetched regularly, with the last good
  copy kept; a price table in Loom's config overrides it, for a model on its first day or a price
  Jann wants to pin.
- Every run uses the latest Claude Code CLI: it updates the CLI in its container before it starts,
  and records the version it used, the way it records the workflow-kit version.

## Options considered

- A price table compiled into Loom: every new model would need a release.
- Anthropic's API: it publishes no prices, and Loom makes no direct API calls (T3).
- Scraping Anthropic's pricing page: it breaks whenever the page changes.
- A CLI version pinned in the worker image: reproducible, but new models and fixes wait for a
  rebuild.

## Consequences

A run that meets a model with no price at all stops with that cause rather than guessing. At the
end of a run, Loom compares its own spend with the cost the CLI reports and logs a warning when
they differ, so a wrong price shows. A CLI update can change the stream format; unknown events are
kept, never dropped, and the recorded version traces a failure to the CLI that caused it.
