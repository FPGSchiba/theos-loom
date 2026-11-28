---
id: 5
type: bug
title: "Cart total ignores removed items"
parent: epic-cart-rules
covers: []
after: []
assignee: ""
refined: true
hitl: false
risk: high
severity: P1
key: SHOP-4
loom: ready
---

# Cart total ignores removed items

## Description

After removing an item the total still counts it. It should drop.

## Reproduction

Add two items, remove one; the total is unchanged.

## Cause Hypothesis

The total is cached before removal.

## Acceptance Criteria

1. **The expected behavior holds**
   **Given** a cart with two items
   **When** one is removed
   **Then** the total drops by its price

## References

- parent — epic-cart-rules/epic-cart-rules.md

## Notes

- Assumption: only the web cart is affected.
