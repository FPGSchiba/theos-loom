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
