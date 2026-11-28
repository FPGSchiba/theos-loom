---
id: 2
type: story
title: "Add an item to the cart"
parent: epic-cart
covers: ["R1"]
refined: true
key: APP-2
needs: [API-2]
---

# Add an item to the cart

## Requirements

1. **Item is added** (R1)
   **Given** an empty cart
   **When** the shopper adds one product
   **Then** the cart shows that product once
2. **Total follows** (R1)
   **Given** a cart with one product
   **When** the shopper adds another
   **Then** the total is the sum of both prices
