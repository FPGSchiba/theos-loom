---
tracker_id: ""
id: 1
type: story
title: "Add an item to the cart"
parent: epic-cart-rules
covers: ["R1"]
sources: []
companions: []
after: [2, "1.3", "epic-pricing-rules"]
assignee: ""
refined: true
hitl: false
risk: low
estimate: 2
key: SHOP-2
loom: ready
needs: [PAY-4]
origin: sliced
---

# Add an item to the cart

## Description

A shopper adds one item and sees it in the cart.

## Requirements

1. **Item is added** (R1)
   **Given** an empty cart
   **When** the shopper adds one product
   **Then** the cart shows that product once

## Constraints

- None beyond the epic.

## Boundaries

- Must not change: product pages

## References

- parent — epic-cart-rules/epic-cart-rules.md

## Notes

- Decision: 2026-01-10, one add path only.
