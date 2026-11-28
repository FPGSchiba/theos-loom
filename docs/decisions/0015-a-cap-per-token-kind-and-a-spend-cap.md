# 0015 · A cap per token kind, and a spend cap

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P03 refinement

## Decision

A run's token caps (TU-03.4) are one optional cap per token kind: input, output, cache write and
cache read. On top, a run may have a spend cap, computed from each kind's tokens and its price per
model (prices from decision 0016). Whichever cap is crossed first stops the run, and the recorded
cause names it.

## Options considered

- One cap on all four kinds summed: simple, but cache reads (most of the tokens, at a tenth of the
  price) would swamp it.
- One cap without cache reads: a looping run that keeps rereading its context would slip under it.
- One cap weighted by price: closest to real usage, but hard to reason about as a token number.

## Consequences

The run records each kind separately, so budgets (P08) and spend (P17) can use the same counts.
With a spend cap set, a model without a price stops the run rather than being guessed.
