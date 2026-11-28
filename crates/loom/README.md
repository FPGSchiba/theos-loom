# loom

Loom's binary for the server and CLI.
For now it has the `plan` commands; with no subcommand it prints its help and exits 0.

## `loom plan check <path>`

Reads the planning repo at `<path>` and checks it. The report goes to stdout, plain text: errors,
then notes, grouped by path, one finding per line, then a count line.

```text
error[broken-reference] components/app/initiative-shop/tickets/epic-cart/epic-cart.md: unknown component "pay"
note[epic-incomplete] components/app/initiative-checkout/tickets/epic-search/epic-search.md:12: the epic has no requirement line
1 error, 1 note
```

`:<line>` is left out when the line is unknown. Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | no errors (notes do not count) |
| 1 | at least one error |
| 2 | `<path>` is not a readable directory (message on stderr) |

## `loom plan schemas <dir>`

Writes the JSON schema of every front-matter type (`<name>.schema.json`) into the existing
directory `<dir>` and lists the files. Exit 0, or 2 when `<dir>` can't be written.

## Logging

`tracing` writes to stderr, default level `warn`; set `RUST_LOG` to change it.
