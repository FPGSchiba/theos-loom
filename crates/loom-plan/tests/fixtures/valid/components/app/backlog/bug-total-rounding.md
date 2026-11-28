---
type: bug
title: "Cart total rounds the wrong way"
severity: P2
key: APP-7
---

# Cart total rounds the wrong way

## Acceptance Criteria

1. **Totals round half up**
   **Given** a cart worth 10.005
   **When** the total is shown
   **Then** it reads 10.01
