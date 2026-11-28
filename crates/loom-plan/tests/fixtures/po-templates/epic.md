---
tracker_id: ""
type: epic
title: "Shoppers manage their cart"
parent: initiative-storefront
covers: ["R1", "R3"]
sources: []
companions: []
after: ["epic-pricing-rules"]
status: in-progress
refined: true
risk: medium
key: SHOP-1
loom: ready
needs: [PAY-4]
---

# Shoppers manage their cart

## Description

A shopper adds, changes and removes items on the deployed site and the total follows.

## Outcome

Shoppers see an accurate cart total at all times.

## Requirements

R1 (parent R1): A shopper can add an item to the cart.
R2 (parent R3): The total follows every change to the cart.

## Constraints

- Totals come from the pricing function.

## Done when

- The cart works on production.

## Boundaries

- Touch point: pricing — the total call; owner: epic-pricing-rules
- Must not change: product pages

## References

- parent — initiative-storefront/initiative-storefront.md, section Requirements

## Notes

- Open question: are guest carts kept?
