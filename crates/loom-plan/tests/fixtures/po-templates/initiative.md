---
tracker_id: ""
type: initiative
title: "Shoppers buy online"
sources: ["docs/prd.md"]
companions: ["architecture-checkout/architecture-checkout.md"]
after: []
status: in-progress
risk: medium
---

# Shoppers buy online

## Description

A shopper finds, prices and pays for an order on the live site.

## Outcome

Shoppers complete a purchase without help.

## Requirements

R1 (prd FR-1): A shopper can pay for a cart.

## Constraints

- Card data never touches our servers (prd NFR-2).

## Done when

- Orders are paid on production.

## Boundaries

- Must not change: the product catalog.

## References

- prd — docs/prd.md, section Requirements

## Notes

- Assumption: one currency only.
