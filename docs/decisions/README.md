# Decision log

Decisions taken by Jann. Each one is a numbered record (`NNNN-<slug>.md`) added in a PR that waits
for Jann. During the trial these include his answers in the decider's place; later, Loom writes
its precedents the same way (umbrella §5.4).

| Date | Decision |
|---|---|
| 2026-11-27 | Umbrella spec version 6 accepted: Loom is built by hand with its own loop as the trial (stage 1), then the editor is onboarded, then the rest. |
| 2026-11-27 | Agents write all code, engine included; Jann reviews after merge. PRs merge on green except for protected paths. |
| 2026-11-27 | A Loom GitHub App is the only identity that posts the review, contract and approval checks; commits stay under Jann's identity. |
| 2026-11-27 | The store is PostgreSQL on Jann's dedicated database VM; tests use a throwaway database. |
| 2026-11-27 | The workflow kit is a BMad fork from v6.12.0 with superpowers replacing BMad's implementation phase. |
| 2026-11-27 | CI on GitHub-hosted runners; theos-loom, theos-editor and Theos-Engine go public: [0001](0001-hosted-runners-and-public-repos.md). |
| 2026-11-27 | MIT licence for theos-loom, theos-editor and Theos-Engine: [0002](0002-mit-licence.md). |
| 2026-11-27 | A check is a required status check; TU-00.2 holds as written: [0003](0003-a-check-is-a-required-status-check.md). |
| 2026-11-27 | The local gate runs on Windows and Linux; CI on Linux only: [0004](0004-local-gate-on-windows-and-linux.md). |
| 2026-11-27 | The toolchain's default linker (`rust-lld`) instead of `mold` with `clang`: [0005](0005-default-linker.md). |
| 2026-11-27 | Every config that can switch off `ui` is protected; `lints` refuses a nested ESLint config: [0006](0006-protect-every-web-tool-config.md). |
| 2026-11-27 | No inline suppressions in `secrets` (`gitleaks:allow`) and `ui` (`eslint-disable`): [0007](0007-no-inline-suppressions-in-secrets-and-ui.md). |
| 2026-11-28 | The final review loop judges every finding, stops when none is worth fixing, and after 5 rounds ends as best effort with one last fix and a targeted review: [0008](0008-judged-review-loop.md). |
| 2026-11-28 | Review fixes inside a PR that already waits for Jann may change protected paths without asking first: [0009](0009-review-fixes-in-a-protected-pr.md). |
| 2026-11-28 | Runs reach the public web through a logging proxy, for research; the home LAN and private ranges stay blocked: [0013](0013-open-web-for-research.md). |
