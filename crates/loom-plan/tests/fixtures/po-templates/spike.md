---
id: 4
type: spike
title: "Which payment provider fits"
parent: epic-checkout
covers: []
after: []
assignee: ""
refined: true
hitl: true
risk: medium
key: SHOP-3
loom: paused
---

# Which payment provider fits

## Description

Two providers are candidates; checkout waits on the answer.

## Approach

1. Compare fees and card support.
2. Prototype a test payment.

## Acceptance Criteria

1. **The candidates are compared on fees and support**
   Verify: docs/payment-comparison.md
2. **The answer is recorded where checkout can find it**
   Verify: docs/decisions

## References

- parent — epic-checkout/epic-checkout.md

## Notes

- Open question: do we need invoices?
