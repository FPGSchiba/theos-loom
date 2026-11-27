# web

Loom's cockpit: React, TypeScript, Vite, pnpm. It starts from Vite's `react-ts` template, trimmed.

The gate checks it with `cargo xtask ui`, which runs fixed `pnpm exec` calls in this directory
(`tsc -b`, `eslint . --max-warnings 0`, `vitest run`) and never a `package.json` script. Node
and pnpm are pinned in `gate/tools.toml`; `.nvmrc`, `engines.node` and `packageManager` here
must equal those pins.
