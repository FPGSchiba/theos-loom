# theos-loom — umbrella spec

Status: approved by Jann · version 8 · 27 Nov 2026 (version 1, the same day: first draft;
version 2 adds epic slicing and the two-phase story run: refinement, then a fresh implementation;
version 3 adds the board, story and epic pages, and short summaries: §7 item 0, P20;
version 4 adds a manual trial and a two-stage build; version 5 removes the review brake again
and makes building Loom itself the trial (§9, O0), so the editor is onboarded only afterwards;
version 6 moves the store from SQLite to PostgreSQL on Jann's dedicated database VM: T1, T4, T6;
version 7, 28 Nov 2026, makes the review loop judge its findings and end as best effort: TU-07.1, decision 0008;
version 8, 28 Nov 2026, opens runs to the public web for research, with the home LAN blocked: T4, decision 0013)
Repo: `FPGSchiba/theos-loom`. A loom weaves many threads in parallel into one fabric.
Owner of architecture: Jann · Built by: Claude Code with superpowers

Loom is a self-hosted harness. It runs Claude Code agents on Jann's projects day and night,
following the work Jann plans in BMad. This spec defines the build as phases. Each phase runs one
superpowers cycle, the same as in theos-editor:
**phase spec → plan → subagent-driven build → final review loop → PR**.

**Precedence:** this file > `docs/architecture.md`, `docs/gate.md`, `docs/repo-contract.md`,
`docs/workflow-kit.md` > phase spec (`docs/specs/phases/`) > plan (`docs/plans/`). A phase spec
adds detail and never changes an acceptance criterion (AC) defined here. Changing an AC means
editing this file, and that needs Jann's approval.

---

## 1. Goal

Jann has about 5–10 hours a week for Fire Phoenix Games. He wants the work to keep moving while
he sleeps or does other work, **without losing quality**.

Loom turns plans into merged, reviewed, tested code:

- Jann plans in Claude Code with a forked BMad (vision, requirements, UX, architecture, epics
  and stories) in one **planning repo** per project.
- When he merges an epic marked `ready`, Loom's **orchestrator** picks it up. Its **decider**
  runs an intake review. Then **workers** build the stories in parallel across the project's
  repos, using superpowers.
- Every change passes layered **quality gates** enforced in code and merges when the checks are
  green. Jann reviews merged work afterwards, in his own time.
- What the vision and directives can't settle comes to Jann as a self-contained decision: the
  full context, the options and a recommendation with its reason, with no internal numbering.
- A web **cockpit** opens on a kanban **board** of every epic and story, with short summaries,
  and shows what happens and what happened, plus decisions, review, controls and spend. There is no chat: the planning repo is the only input, and the command bar is for
  steering.

Loom is built for **many projects** from the start. Theos is the first.

### 1.1 Non-goals

- Not a planning tool. Planning stays in Claude Code. Loom never writes vision, directives or
  epics. It writes stories only by slicing an epic Jann made `ready`, and it *proposes*
  follow-up stories as PRs he merges.
- Not a multi-user product. One user (Jann), one Claude subscription, one home server
  (constraint T2).
- Not a chat with the agents.
- No creative content. Per project, the directives say which content agents may never touch.
  For Theos that means lore, design concepts, world content and art (R7 in §6.2, O8).

### 1.2 Anti-goals (a failure even if it technically works)

- Speed bought with quality: a gate weakened, skipped or worked around to get a merge through.
- A decision taken for Jann that the vision, directives and precedents don't actually support,
  or one he can't trace afterwards.
- An escalation Jann can't act on without opening five files: it lacks context, uses internal
  IDs or has no recommendation.
- A night's usage burned on a stuck worker, or a morning where Jann's own Claude sessions have
  no usage left.
- State that only exists in a model's context, so that a crash or restart loses it.

---

## 2. Concepts

| Term | Meaning |
|---|---|
| **Project** | Top isolation level. One planning repo, one orchestrator, one **vision**, optional project-wide directives. Example: Theos. |
| **Component** | A product inside a project, with its own repo and one parent **initiative** (its BMad brief/PRD and architecture). Holds the component's **engineering directives** and **design directives**, which add to the project-wide ones and never contradict them. Theos has three: Engine, Editor, Tower of Theos. |
| **Epic** | A slice of a component's initiative, planned with BMad: goal, scope, out of scope, and epic-level acceptance criteria. It becomes work when Jann merges it with status `ready`. Its stories may be written by Jann or sliced by Loom. |
| **Story** | The unit of work for one worker: one branch, one PR, in one component repo. It carries its acceptance criteria and its dependencies, which may point into other components. |
| **Orchestrator** | Deterministic Rust code: reads the planning repo, runs the state machines, schedules, enforces gates and budget. It holds no model context. |
| **Decider** | A short Claude Code run that the orchestrator starts for each judgment call. It gets a **briefing**, **retrieval** and **read-only tools** (§5). It decides, or it escalates. |
| **Worker** | A Claude Code run in its own container and git worktree. There are three kinds. A **slicer** cuts an epic into stories (BMad's PO workflow). A **refiner** brainstorms one story with the decider as its partner and writes its design and plan. An **implementer** is always a fresh run that builds a story from its approved design and plan. |
| **Reviewer** | A fresh Claude Code run that didn't write the change. It reviews it against the story, the directives and the vision. Its verdict is a required check. |
| **Precedent** | A past decision by Jann, with its reasons, stored as a decision record and retrieved for similar cases. |
| **Workflow kit** | The forked BMad (planning phases) plus superpowers with an unattended overlay (implementation), pinned per project (§4). |
| **Repo contract** | The rules every component repo must meet before it is onboarded, and keep meeting afterwards (§6). |
| **Cockpit** | The web app: board with story and epic pages, activity log, decision inbox, review queue, controls and command bar, spend. |

### 2.1 Planning repo layout (per project)

```text
README.md
loom.toml                       # project id, components, repos, budget weights, notification target
vision.md                       # the project vision (Jann)
directives/
  project.md                    # project-wide engineering and design directives (optional)
components/<component>/
  component.toml                # repo URL, default branch, workflow-kit version, forbidden paths
  initiative/                   # BMad brief, PRD, UX, architecture for this component
  directives/engineering.md
  directives/design.md
  epics/<epic-slug>/
    epic.md                     # front matter: status, priority, depends_on; goal, scope, epic ACs
    stories/<story-slug>.md     # written by Jann or sliced by Loom; status, depends_on, ACs
canon/                          # design and canon the workers build from (Theos: O4)
decisions/NNNN-<slug>.md        # decision records, Jann's answers and precedents
archive/                        # read-only imports (Theos: the Jira backlog)
_state/                         # written only by Loom (§3.2); everything else is Jann's
```

---

## 3. How work flows

### 3.1 Life of an epic

1. **Plan.** Jann plans in Claude Code with the workflow kit's planning skills, down to the
   epic: goal, scope, out of scope and epic-level ACs. Writing stories himself is optional. He
   merges the epic with `status: ready`.
2. **Detect.** The orchestrator polls the planning repo's default branch. A webhook can be added
   later. It sees the new or changed `ready` epic.
3. **Intake.** The decider reviews the epic before any worker starts. It checks against the
   vision and directives, against running work (conflicts, overlapping files), for gaps and
   ambiguities, and for missing or circular dependencies. It also checks whether a story would
   touch a forbidden path. Outcomes: *accepted* (work order computed), *accepted with notes*
   (notes logged), or *questions*, which become escalations, and the epic waits for the
   answers. Work that doesn't depend on the questions may start.
4. **Slice** (only for an epic without stories). A slicer run cuts the epic into stories with
   ACs and dependencies, and opens them as a PR to the planning repo. The decider reviews that
   PR against the epic. The component's `slicing` setting decides what happens next: `auto`
   (default) means Loom's service merges it once the decider approves, and `review` means it waits for Jann.
   Stories Jann wrote are taken as they are. The decider's review of the slicing PR is the
   **story intake**: it checks forbidden paths, overlaps and dependencies per story, the same
   as step 3 does for written stories. Changes that Loom itself merged never trigger a new
   epic intake.
5. **Schedule.** Stories become runnable once their dependencies are merged, then run in an
   order set by priority, budget and conflict avoidance (§3.3).
6. **Refine** (per story). A refiner run brainstorms the story with superpowers. Each question
   starts its own short decider run, with the briefing plus the epic's earlier questions and
   answers. The decider answers, and the refiner continues. If the decider escalates, the refiner
   commits a checkpoint (its notes and the question log) to the story branch and **ends**. When
   Jann answers, a new refiner run resumes from that checkpoint. No container waits idle, and
   time spent waiting for the decider doesn't count toward stuck detection. The refiner writes the design and the plan on the
   story branch, and the decider approves both against the story, the directives and the vision
   (at most 3 rounds, then it escalates). Brainstorming scales with the story: a small story gets
   the short form. The refinement run ends here.
7. **Implement** (per story). A **fresh** implementer run gets only the story, the approved
   design and plan, the directives and the repo. It builds with subagent-driven development and
   opens the PR. A gap in the plan, or any other blocker, goes to the decider and is never
   quietly patched over. A plan change the decider allows is committed to the plan file, so the
   PR shows it.
8. **Gate.** CI checks, the contract checks and the independent review loop (§6.2) run. Fixes go
   back to the worker. Green means the PR merges. A PR that touches a protected path waits for
   Jann.
9. **Review queue.** The merged change joins Jann's review queue. If he flags it, Loom opens a
   planning-repo PR with a follow-up story (a fix, a revert or a change), marked `ready` if he
   says so, and the decider offers to turn his comment into a directive or a check.
10. **Close.** When every story of an epic has merged, the epic is `done`. When all epics of an
   initiative are done, Loom reports it. That's the hook for a later sync to Confluence (§10).

### 3.2 Who writes what

- Story files that Loom sliced carry `origin: sliced` in their front matter. Loom's service (not
  a worker) opens the slicing PR, and with `slicing = auto` merges it once the decider approves.
  These files stay Loom's until Jann edits one. From then on it's his, and Loom changes it only
  by a PR he merges.
- **Workers never hold a planning-repo token.** Loom's service copies the files a run needs into
  the container, read-only: the story and epic, the directives, the vision, and the canon the
  component uses. The slicer writes its stories to an output folder, and Loom's service turns
  them into the PR.
- Jann owns every other file in the planning repo except `_state/` and `decisions/`. Loom pushes
  straight to `main` only in those two: `_state/` mirrors, and new decision records (append-only,
  never edited). Everything else Loom wants in the planning repo (follow-up story drafts, a
  proposed directive or check text) goes as a **PR that Jann merges**. So T11 holds: Loom never
  changes his files without his merge.
- Status a person sets lives in front matter: `draft`, `ready`, `paused`, `dropped`.
- **Jann's actions in the cockpit are his edits.** Moving a card to `ready`, `paused` or `dropped`
  on the board, or changing its priority, commits that one front-matter field to the planning
  repo under his identity, with a `Via: loom-cockpit` trailer. These are the only planning
  files the cockpit writes. Everything else is still edited in Claude Code.
- Run state lives in Loom's database and is mirrored to `_state/<component>/<epic>.toml` on each
  change. Epics: `intake`, `slicing`, `active`, `blocked`, `done`. Stories: `queued`, `refining`,
  `refined`, `implementing`, `in-review`, `blocked`, `merged`. That way git
  shows the state too, and Jann's edits never conflict with Loom's.
- Any action that changes direction (a command, an answer, a flag in the review queue) leaves a
  record in git. Git plus Loom's database are the whole state. No model context is relied on
  across runs.

### 3.3 Scheduling and budget

- Loom signs in to Claude Code with Jann's **personal** subscription. Every worker, reviewer and
  decider run draws from its usage limits (T3).
- **Budget windows** (configurable): caps Jann sets on the tokens Loom itself spends per
  window. A generous night cap, and a small day cap that leaves room for his own interactive
  sessions. The CLI doesn't report how much subscription usage is left, and Loom can't see
  Jann's own sessions. So the caps plus backing off on rate limits make a morning without usage
  much less likely, but can't guarantee it. The spend page says so.
- When the CLI reports a rate limit, Loom pauses new runs, keeps the paused ones resumable and
  restarts when the limit resets. It never retries in a tight loop.
- **Fair share across projects** by weights in each `loom.toml`. Priority inside a project comes
  from the epic's and story's `priority`.
- **Concurrency limits**: global, per project and per component.
- Budget and concurrency limits count every run: slicers, refiners, implementers, reviewers and
  deciders.
- **Conflict avoidance** applies to implementers. The refiner's plan lists the files and areas
  the story will touch. Two implementers with overlapping areas in one repo don't run at the
  same time unless the decider rules that they don't conflict. Refiners only read, so they may
  overlap.
- **Stuck detection**: a run with no progress for a set time, or over its token or time cap, is
  stopped. The story retries once from a clean worktree, then goes to the decider.

---

## 4. Workflow kit

- **BMad fork.** `FPGSchiba/bmad-method`, forked from the latest stable release (v6.12.0 when
  this spec was written). Changes from `next` or a later upstream release are pulled
  deliberately, as their own story, never automatically. Phases 1–3 stay BMad: analysis, PRD,
  UX, architecture, and the PO's slicing into epics and stories. The fork changes the story
  template to the front matter Loom reads (§2.1).
- **Phase 4 is superpowers.** BMad's implementation chain (sprint planning, build, build-auto,
  code-review) is replaced by superpowers: writing-plans → subagent-driven-development →
  requesting-code-review → finishing-a-development-branch.
- **Unattended overlay** (a plugin in the kit, like the overrides in theos-editor's
  `CLAUDE.md`). Wherever a skill says "ask your human partner", the worker asks the decider
  through the `ask_decider` tool. Wherever it says "get approval", the decider approves.
  - **Slicer:** BMad's PO workflow, writing stories in Loom's front matter.
  - **Refiner:** superpowers brainstorming, the design doc, then writing-plans (with the
    story's ACs copied verbatim). The paths come from `component.toml`, with the defaults
    `docs/specs/stories/{story}.md` and `docs/plans/{story}.md`. The editor keeps its
    `docs/specs/phases/` and `docs/plans/` layout this way. Both are committed on
    the story branch. The builder check of brainstorming stays: a fresh subagent reads the design
    before the decider sees it.
  - **Implementer:** always a new run, with no history from refinement. It reads only the files
    above. It runs subagent-driven development, keeping its per-task reviews.
  - The whole-branch final review does **not** run inside the implementer. There is exactly one
    final loop, Loom's review service (TU-07.1), so no story pays for two.
  - Finishing is always a PR with auto-merge, unless it touches a protected path.
  - Every decider answer given during an epic is stored and offered to later runs of the same
    epic, so the same question isn't asked twice.
- **Pinning.** Each component names its kit version in `component.toml`. The version shows on
  every run in the activity log, so a failure can be traced to the kit that was in use.

---

## 5. The decider and its context

### 5.1 When it runs

Intake of an epic. Review of a slicing PR. Questions during brainstorming, and approval of a
refiner's design and plan. A blocker raised by a worker. A review loop that hit its cap. A main branch
that turned red after a merge. A free-text command (§7, item 4). A conflict ruling between two implementers.
Turning Jann's review flags into proposed directives or checks.

### 5.2 Briefing (always included, built by code)

1. The project vision and the project-wide directives.
2. The component's engineering and design directives, and its initiative summary.
3. The epic and the story in question, with ACs and dependencies.
4. The trigger: what the worker was doing, the question or failure, and the relevant log
   excerpt.
5. Live state: other runs in the project, their stories and touched areas, open PRs, recent
   merges, and red checks.

### 5.3 Retrieval and tools

- **Index** per project over the planning repo (vision, directives, initiatives, canon,
  decisions, archive) and over each component repo's docs. Chunks keep their path and heading.
  Search is hybrid: keyword (BM25) plus local embeddings. No cloud embedding service (T9).
- **Precedents first.** Decision records are ranked above other sources, and the briefing always
  includes the top matches with Jann's reasons.
- **Read-only tools** through an MCP server that Loom runs: `state_query`, `search`,
  `read_file` (planning and component repos), `run_log`, `pr_status`. Nothing in this toolset can
  change anything.

### 5.4 Deciding and escalating

- The decider returns structured output: `decide` (choice, reasoning, sources, confidence) or
  `escalate` (the escalation fields below). It may decide alone only when a directive, the
  vision or a precedent supports the choice and its confidence is at or above the project's
  threshold. Otherwise it escalates.
- **Strict start.** A new project starts with a high threshold. Jann lowers it in `loom.toml`.
  Loom shows him, per threshold, how many past decisions would have gone each way, but never
  changes the threshold itself.
- **Every decision** is logged with its reasoning and the exact sources it used, and is visible
  in the activity log.
- **An escalation** has: a title in plain words; *what's going on* (the full context, written
  for someone who hasn't followed along); *why it can't be decided alone*; two to four
  *options*, each with what it costs and what it gives; a *recommendation and the reason for
  it*; and *what is blocked meanwhile, and what continues*.
- **No internal numbering in escalations.** Things are named by their titles and described in
  plain words, never by story or epic slugs, AC ids, phase numbers, decision numbers, branch
  names, commit hashes or file paths. Paths are allowed only in a collapsed "details" part. An
  escalation that breaks this rule is rewritten before it's sent (TU-12.4).
- **Answers** are a choice plus an optional comment. Loom commits them as a decision record
  (`decisions/NNNN-<slug>.md`, kind `answer`) with the question, the options, the choice and
  the reason, which makes them precedents. Then the blocked work resumes.
- While an escalation is open, the orchestrator keeps working on everything that doesn't depend
  on it.

---

## 6. Quality gates and the repo contract

### 6.1 Layers

1. **The repo's own checks in CI.** Build, tests, lints and formatting, as the repo declares them
   in `.loom/checks.toml`.
2. **Contract checks** (`loom-contract`): AC traceability, forbidden paths, protected paths,
   PR form, commit form, and no weakening (§6.2). Loom's service computes them, because it can
   read the story's ACs in the planning repo, and posts the result as a check. Component repos
   hold no token to the planning repo, and a copy of the ACs written by a worker is never trusted.
3. **Independent review**: a fresh reviewer run, posted as the required status check
   `loom-review`. A worker can never set it.
4. **Jann's review after merge**, in the review queue.

### 6.2 Contract rules (every component repo, `docs/repo-contract.md` in detail)

- **R1 README and agent instructions.** A README, and a `CLAUDE.md` that holds the repo's
  commands, conventions and directives, or points to them in the planning repo.
- **R2 Declared checks.** `.loom/checks.toml` lists every check and the command that runs it
  locally with the same verdict as CI. A repo without declared checks can't be onboarded.
- **R3 Required checks.** Branch protection on the default branch: every declared check plus
  `loom-contract`, `loom-review` and `loom-approval` are required status checks, the last three
  pinned to the Loom GitHub App. Squash merge only, linear
  history, branches up to date before merging, no direct, force or delete pushes. Loom reads
  these settings and reports any drift.
- **R4 Tests prove ACs.** Every AC of a story has at least one test (or, for documentation-only
  stories, a check) tagged `AC: <id>`. No tag names an unknown AC.
- **R5 Protected paths** are listed in `.loom/protected.toml`: the gate configuration, CI
  workflows, `checks.toml`, the agent instructions, lint, toolchain and dependency policy files.
  A PR that touches one waits for Jann. This is the only case in which Loom doesn't merge on
  green. Jann approves it in the cockpit (or with `loom approve`). The App then posts
  `loom-approval`. Labels are only a visible hint and never the gate, because a worker's token
  could set a label. On a PR without protected paths, the App posts `loom-approval` as passed right
  away.
- **R6 No weakening.** `loom-contract` fails when a PR deletes or skips a test, lowers a
  threshold, adds a lint exception without a reason, or edits a protected path without Jann's
  approval (`loom-approval`). Projects may add language-specific rules. For Rust repos, theos-editor's
  `lints` check is the model.
- **R7 Forbidden paths.** `component.toml` may list paths no agent may change, such as Tower of
  Theos' lore, design and art. `loom-contract` fails any PR that a Loom run created and that
  touches them. Loom knows its PRs from its own run records, not from the author, which is
  Jann in both cases. Jann's own PRs are not affected.
- **R8 Conventional Commits** and PR titles, with the Loom trailers that name the story and run.
- **R9 Reproducible.** Pinned toolchain, committed lockfile, builds with `--locked` or the
  language's equivalent.
- **R10 Secrets.** No secrets in the repo. A secret scan runs in CI.
- **R11 Main stays green.** If the default branch turns red after a merge, Loom pauses that
  component, opens a revert PR for the merge that caused it (or a fix, if the decider is sure),
  and notifies Jann.
- **R12 Hardware-bound tests.** Tests that need a GPU or another device are tagged (for Rust: a
  `gpu_` name segment, like the editor's `slow_`). They're left out of the required checks and run
  on a schedule where the hardware exists. `wgpu` code gets a software-adapter test path
  (lavapipe/llvmpipe) inside the required checks, so the renderer isn't untested on every PR.

### 6.3 Audit and drift

`loom audit <repo>` checks R1–R12 and writes a report. Onboarding a component needs a passing
audit. Loom re-audits every component nightly. Drift pauses the component and creates a
compliance story. Because such stories touch protected paths, they wait for Jann.

---

## 7. Cockpit

React (Vite, TypeScript) talks to the Axum API over REST, with live updates over Server-Sent
Events. One user. Reached only through Jann's VPN or reverse proxy, with its own login (T8).

0. **Board** (the start page): a kanban view of every epic and story, Jira-style, built from
   the planning repo and Loom's state. Columns follow the states: *Draft*, *Ready*, *Slicing*,
   *Queued*, *Refining*, *Implementing*, *In review*, *Merged*, plus a collapsed *Paused / Dropped*
   area at the end. *Blocked* is a red flag on the
   card, not a column, so a card keeps its place. Swimlanes by component (or by epic), with
   filters by project, component, epic and status. A card shows the title, a one-line summary,
   its acceptance criteria as done/total, and a marker when a decision is waiting for Jann.
   Dragging a card works only for Jann's own moves (to *Ready*, *Paused*, *Dropped*, and order
   within a column for priority). Every state Loom owns is shown, never set by hand (§3.2).
   - **Story page:** a short **summary** in plain words (what it does, why, what it touches),
     the **acceptance criteria verbatim** (they're binding, so they're never summarised),
     dependencies, and the epic. Tabs: *Activity* (this story's runs in order: slicing,
     refinement with every question and answer, implementation, review rounds, merge),
     *Decisions* (the decider's rulings with their reasons, and Jann's answers), *PR and
     checks*, *Spend*, and *Source* (the full BMad story, design and plan, for when he wants
     the detail).
   - **Epic page:** summary, epic-level ACs verbatim, its stories as a mini board, progress, and
     the open decisions.
   - **Summaries** are made by a small summary run whenever a story's or epic's file changes.
     They're stored in Loom's database with the hash of the file they came from, and marked
     as summaries. They're never written to git and never used as input for any run: workers
     and the decider always read the real files. A summary whose source changed is redone
     before it's shown again.
1. **Activity log**: live and history, filterable by project, component, epic, story and run.
   For each run: its step, its last actions, linked PRs and checks, the decider's decisions with
   their reasoning and sources, and the workflow-kit version.
2. **Decision inbox**: open escalations in the format of §5.4. Answering one commits the decision
   record. Answered decisions stay searchable.
3. **Review queue**: merged changes awaiting Jann's look, sorted by risk (size, area, whether
   they touched code the directives call sensitive), then by age. Actions: *looks good*,
   *flag* (with a comment, which creates a follow-up), *revert*.
4. **Controls and command bar**: pause or resume all, a project or a component; reprioritise,
   re-run, cancel; budget windows. Free-text commands go to the decider, which proposes concrete
   actions and runs them only after Jann confirms. Commands that change direction leave a record
   in git (§3.2).
5. **Spend**: tokens and API-equivalent cost as the CLI reports them per run, aggregated by
   story, epic, component, project and day. Usage against the budget windows, rate-limit
   events and pauses. Cost per merged story over time.

Push notifications for new escalations, for a main branch that turned red, and for a paused
component. The default is a self-hosted ntfy topic (my call).

---

## 8. Global constraints

- **T1** Rust stable, pinned, edition 2024; `unsafe_code` forbidden; `Cargo.lock` committed;
  builds `--locked`. Crates: `loom-core` (domain, state machines), `loom-plan` (planning repo
  reader and validation), `loom-store` (PostgreSQL through `sqlx`, migrations with `sqlx migrate`), `loom-run` (containers,
  worktrees, Claude Code runs), `loom-gate` (contract checks and audit), `loom-context`
  (index, briefings, MCP tools), `loom-api` (Axum) as libraries; `loom` (server and CLI) and
  `xtask` as binaries. The UI lives in `web/` (React, TypeScript, pnpm).
- **T2** One user, one subscription. Loom is Jann's personal tool and is never offered to
  others with his Claude login.
- **T3** Models run only through the Claude Code CLI in headless mode (`claude -p` with streamed
  JSON output), signed in with Jann's personal plan in the run containers. No direct API calls
  from Loom. Switching to an API key later is a config change, not a code change.
- **T4** Loom runs in a dedicated VM on the home server (not an LXC, so rootless nested
  containers and per-container egress filtering work). Every worker and reviewer runs in its
  own rootless container with its own git worktree. Inside a run, subagents share that worktree
  only when they run one after another; subagents that write in parallel each get their own
  worktree and branch, merged back one at a time. Network only to public HTTP(S)
  destinations, through a proxy that records every destination in the run's log, so research
  tools work (decision 0013). No access to the home LAN, the database VM or any other private
  address range. Credentials: a
  fine-grained GitHub token scoped to the project's repos (contents and pull requests only).
  Commits and PRs appear under Jann's GitHub identity, as in theos-editor.
- **T4a GitHub App.** A Loom GitHub App, installed on the project's repos, is the only identity
  that posts `loom-review`, `loom-contract` and `loom-approval`. Branch protection pins these
  required checks to the App as their source, so nothing posted with Jann's token, and so
  nothing a worker posts, can satisfy them. The App never commits and never opens PRs. Its
  private key lives only in Loom's secret store, never in a container.
- **T4b Database.** Loom's state lives in PostgreSQL on Jann's dedicated database VM, in its own
  database and role. Only Loom's service reaches it. Worker containers never do, and the
  database's credentials never enter a container. Tests and CI use a throwaway PostgreSQL (a CI
  service container, or one started by `cargo xtask`), never the real database.
- **T5** Git only through the `git` command line; GitHub through `gh` or its REST API.
- **T6** All state is in git and PostgreSQL. Restarting Loom at any moment loses at most the
  running steps, which resume or retry from their last checkpoint.
- **T7** Library errors are typed (`thiserror`); `anyhow` only in binaries; logging with
  `tracing`. Every run's raw stream is kept for 30 days.
- **T8** Single sign-in for the cockpit (a passkey or password with a session cookie). Never
  exposed to the internet without the VPN or a reverse proxy in front. Secrets only in the
  server's secret store or environment, never in the planning repo.
- **T9** No telemetry. Embeddings and the index run locally.
- **T10** Escalations never contain internal numbering (§5.4).
- **T11** No worker, reviewer or decider run weakens a gate or posts a Loom check (only the App
  does, T4a). Loom never merges a PR that touches a protected path without `loom-approval`, and
  never edits Jann's planning files. Moves Jann makes on the board are his own edits (§3.2).
- **T12** Every AC below has at least one test tagged with its id. Loom's own repo meets the
  repo contract (§6.2), with theos-editor's gate as the model.
- **T13** The engineering directives of theos-editor's `CLAUDE.md` (maintainability first, use
  what exists, change the approach when fixes keep leaking, the gate is not a fortress) apply to
  Loom's own build.

---

## 9. Phases

Slices say what Jann can do once their phases are merged.

| Phase | Name | Slice | Depends on |
|---|---|---|---|
| P00 | Foundation and gate | S1 | – |
| P01 | Planning repo model | S1 | P00 |
| P02 | Store and state machines | S1 | P01 |
| P03 | Run runtime | S1 | P00 |
| P04 | Workflow kit | S1 | – |
| P05 | Story loop end to end | S1 | P02, P03, P04 |
| P06 | Repo contract and audit | S2 | P00 |
| P07 | Merge pipeline and review check | S2 | P05, P06 |
| P08 | Scheduler and budget | S3 | P05 |
| P09 | Multi-project | S3 | P08 |
| P10 | Context service | S4 | P02 |
| P11 | Decider | S4 | P05, P10 |
| P12 | Escalations and decision records | S4 | P11 |
| P13 | API and live stream | S5 | P02 |
| P14 | Cockpit shell and activity log | S5 | P13 |
| P15 | Decision inbox and review queue | S5 | P12, P14 |
| P16 | Controls and command bar | S5 | P11, P14 |
| P17 | Spend | S5 | P08, P14 |
| P18 | Onboarding toolkit | S6 | P06, P09 |
| P19 | Deployment on the home server | S6 | P05 |
| P20 | Board, story and epic pages | S5 | P13, P14 |

**Build order.** Loom is built in three stages. Each starts only if the one before earns it.

1. **Stage 1, the trial: Loom builds itself by hand.** The phases of the first version (P00–P05,
   P06, P07, TU-08.1 and TU-08.2 of P08, P13 and P14) are built with the same two-phase loop Loom
   will later run on its own, but driven by hand in Claude Code (O0). This both builds Loom and
   is the test of whether the loop's output is good enough. The decider stays the P05 stand-in,
   with escalations shown in the activity log.
2. **Then the editor.** Once the first version runs, the editor is onboarded (O1–O6, O9) as
   Loom's first real component, and Loom itself as its second.
3. **The rest, if the first version proves out.** The full decider and context (P10–P12),
   scheduler and budget (rest of P08), the board (P20), inbox and review queue (P15), controls
   (P16), spend (P17), multi-project (P09), onboarding of the engine and Tower of Theos (P18,
   O7, O8), and deployment polish (P19), all built by Loom itself.

Jann decides between the stages, from O0's numbers and his own judgement of the code.

Slices: **S1** one story runs unattended from a planning repo to a PR · **S2** it merges safely
· **S3** many stories, many projects, inside the budget · **S4** it decides or asks well ·
**S5** Jann sees, navigates and steers everything · **S6** a new project goes from nothing to running.

P00, P03, P04 and P06 can run in parallel. The UI phases start after P13.

---

### P00 · Foundation and gate

**TU-00 Workspace, CI and gate**
- TU-00.1 The workspace with the crates of T1 (empty, documented) and `web/` builds and tests in
  CI on Linux.
- TU-00.2 Every check of `docs/gate.md` is a required status check on `main`. Branch protection
  matches R3.
- TU-00.3 `cargo xtask gate` gives the same verdicts as CI.
- TU-00.4 `ac-trace`, `pr-title`, `commit-msg`, `lints` and `approval` behave as in
  theos-editor's gate, until P07 replaces `approval` with `loom-approval`.

### P01 · Planning repo model

**TU-01 Reading and validating a planning repo**
- TU-01.1 `loom-plan` parses every file of §2.1 into typed models. Unknown front-matter keys are
  errors that name the file and the key.
- TU-01.2 `loom plan check <path>` reports broken references (unknown component, epic or
  story), dependency cycles, stories without ACs, epics without a goal, scope or epic-level
  ACs, and duplicate ids. An epic without stories is valid (it gets sliced). It exits non-zero on any
  of them.
- TU-01.3 Directives resolve by layer (project, then component). A component directive marked as
  contradicting a project directive is reported.
- TU-01.4 A JSON schema for every front-matter type is generated from the models and shipped,
  so the BMad fork's templates validate against it.

### P02 · Store and state machines

**TU-02 State**
- TU-02.1 The epic and story state machines of §3.2 are encoded as types. An illegal transition
  is unrepresentable or a typed error, tested for every pair of states.
- TU-02.2 Every transition is an event in PostgreSQL with its time, cause and actor (Loom, decider
  or Jann). Current state can be rebuilt from the events.
- TU-02.3 `_state/` mirror files are written on each transition and committed to the planning
  repo, batched at most once a minute. A conflict with Jann's commits never touches his files.
- TU-02.4 After a kill at any point, a restarted Loom reaches the same state as one that wasn't
  killed (crash test over a scripted run).

### P03 · Run runtime

**TU-03 Running Claude Code in a container**
- TU-03.1 `loom-run` starts a rootless container from the configured image, with a fresh git
  worktree of the story branch and the workflow kit at the pinned version. No two runs ever share
  a worktree (tested with two concurrent runs of one story).
- TU-03.2 It runs `claude -p` with streamed JSON output, parses every event into typed run
  events, and stores the raw stream (T7).
- TU-03.3 A run can be cancelled. Cancelling stops the whole process tree and the container
  within 10 s.
- TU-03.4 Token and time caps per run stop it. The cause is recorded.
- TU-03.5 The container's network allows only the destinations of T4 (tested with a blocked
  request).
- TU-03.6 Rate-limit and authentication errors from the CLI become distinct typed events.

### P04 · Workflow kit (repo `FPGSchiba/bmad-method`)

**TU-04 BMad fork and unattended overlay**
- TU-04.1 The fork is based on the latest stable upstream tag. Its README lists every change
  from upstream.
- TU-04.2 The PO's epic and story templates, and the slicer's output, produce files that pass
  `loom plan check`.
- TU-04.3 BMad's implementation skills are removed from the kit's install. A planning session
  that asks for them is pointed to Loom.
- TU-04.4 The overlay routes every question and approval of brainstorming and writing-plans to
  `ask_decider`. No skill in a worker ever waits for a human.
- TU-04.5 The kit has a version, and a worker's activity log shows it.
- TU-04.6 The refiner writes the design and the plan to the paths of §4, with the story's ACs
  copied verbatim into the plan. The implementer has no whole-branch review loop and finishes
  with a PR.

### P05 · Story loop end to end

**TU-05 One story, unattended**
Until P11 lands, a stand-in decider runs these: it approves a design and plan when their builder
check reports nothing important, and approves a slicing PR when `loom plan check` passes. It
answers every question with "decide by the directives and record your assumption in the design",
so a story can pass refinement without input. The real answers arrive with P11.
- TU-05.1 With a sandbox planning repo and a sandbox component repo, a `ready` story with no
  dependencies goes through refinement and implementation and becomes a PR, without any input.
- TU-05.2 The PR's title, body and commits meet R8, and its tests carry the story's AC tags.
- TU-05.3 A worker's question reaches the `ask_decider` tool and gets the stand-in's answer.
  Every assumption it led to is listed in the design.
- TU-05.4 A failed run retries once from a clean worktree, then goes to `blocked` with its
  cause.
- TU-05.5 The implementer is a separate run. Its context contains none of the refinement run's
  messages, only the files of §4 (verified from the run records).
- TU-05.6 An epic without stories is sliced into a planning-repo PR. With `slicing = auto` Loom's
  service merges it once the decider approves. With `review` it waits for Jann.

### P06 · Repo contract and audit

**TU-06 Contract**
- TU-06.1 `docs/repo-contract.md` defines R1–R12 in testable detail.
- TU-06.2 `loom audit <repo>` checks each rule and reports pass or fail with the reason. Each
  rule has a test with a passing and a failing fixture repo.
- TU-06.3 The `loom-contract` check, computed by Loom's service and posted by the App, fails a PR for each violation of R4, R5, R6,
  R7 and R8. Each case is tested.
- TU-06.4 The nightly audit detects drift in branch protection and checks, pauses the component,
  and creates a compliance story.

### P07 · Merge pipeline and review check

**TU-07 Safe merge**
- TU-07.1 After CI is green, a fresh reviewer run reviews the PR against the story, the
  directives and the vision. Every finding is judged: it goes to the worker to fix, or it is
  ruled not worth fixing, with the reason. The loop repeats with a new reviewer until one reports
  nothing worth fixing. After 5 rounds, one last fix and a targeted review end the loop as best
  effort, and the PR lists its open findings.
- TU-07.2 Only the Loom GitHub App posts `loom-review`, `loom-contract` and `loom-approval`.
  Branch protection requires them from the App. A check or status with the same name posted with
  a worker's token doesn't satisfy the requirement (verified against a sandbox repo).
- TU-07.3 A PR without protected paths is enabled for auto-merge and merges on green. A PR with
  protected paths gets `needs-jann` and waits.
- TU-07.4 A red default branch after a merge triggers R11 within one poll interval.
- TU-07.5 Every merge is added to the review queue with its risk score.

### P08 · Scheduler and budget

**TU-08 Scheduling**
- TU-08.1 Dependencies, also those between components, gate when a story may start.
- TU-08.2 Concurrency limits (global, project, component) are never exceeded (property test).
- TU-08.3 Budget windows limit new runs. Loom's token spend never exceeds a window's cap; a
  run that would cross it is not started.
- TU-08.4 A rate-limit event pauses new runs until the reset time. No run starts in between.
- TU-08.5 Two implementers whose planned areas overlap in one repo never run at the same time
  unless a decider ruling allows it.
- TU-08.6 A stuck run (no progress within the configured time) is stopped and handled as in
  TU-05.4.

### P09 · Multi-project

**TU-09 Projects**
- TU-09.1 Several projects run with their own orchestrator state, planning repo and index. No
  data from one project shows up in another's briefing, retrieval or cockpit view (tested).
- TU-09.2 Fair share by weights over a scripted load, within 10 % of the target over a day.
- TU-09.3 Adding or removing a project needs no restart.

### P10 · Context service

**TU-10 Briefing, retrieval, tools**
- TU-10.1 The briefing builder produces §5.2 from state and the planning repo. Its sections and
  size limits are tested, and it states what was cut.
- TU-10.2 The index updates within one poll after a commit to the planning repo or a component
  repo's docs.
- TU-10.3 Hybrid search returns the expected chunks for a labelled query set. Precedents rank
  above other sources when relevant (measured on the set).
- TU-10.4 The MCP tools of §5.3 work read-only. Any attempt to write through them fails
  (tested).

### P11 · Decider

**TU-11 Decisions**
- TU-11.1 Intake runs for every new or changed `ready` epic and produces one of the outcomes of
  §3.1 step 3 as structured output.
- TU-11.2 The decider decides alone only when its output cites a directive, the vision or a
  precedent and its confidence is at or above the threshold. Otherwise its result is an
  escalation (tested with fixtures on both sides).
- TU-11.3 Every decision is stored with its reasoning, its sources and the briefing it saw.
- TU-11.4 The threshold report of §5.4 shows how past decisions would split at a given
  threshold.
- TU-11.5 The decider approves or returns a refiner's design and plan, each time with its
  reasons. After 3 returns it escalates.
- TU-11.6 Answers given during an epic are offered to later runs of that epic (tested: the same
  question in a second run is answered from the stored answer).
- TU-11.7 An escalated question ends the refiner run with a checkpoint on the story branch. A new
  run resumes from it after Jann answers (scripted scenario).

### P12 · Escalations and decision records

**TU-12 Asking Jann**
- TU-12.1 An escalation has every field of §5.4. A missing field fails validation.
- TU-12.2 Answering one commits a decision record and resumes the blocked work within one poll.
- TU-12.3 Work that doesn't depend on an open escalation continues (scripted scenario).
- TU-12.4 The **plain-words check** rejects any escalation whose main text contains an id
  pattern of §5.4: slugs, AC ids, phase or decision numbers, branch names, hashes or paths. The
  decider rewrites it, and after two failed rewrites the escalation is sent with a warning to
  Jann.
- TU-12.5 A push notification goes out for each new escalation.

### P13 · API and live stream

**TU-13 API**
- TU-13.1 Axum REST endpoints for projects, components, epics, stories, runs, decisions,
  escalations, the review queue, commands and spend, described in an OpenAPI document generated
  from the code.
- TU-13.2 An SSE stream sends every event of TU-02.2 and every run event of TU-03.2. After a
  reconnect, it resumes from the last event id.
- TU-13.3 Every endpoint except login needs the session (T8).

### P14 · Cockpit shell and activity log

**TU-14 Seeing**
- TU-14.1 The cockpit lists projects and components with their live state.
- TU-14.2 The activity log shows runs live and as history, with the details of §7.1 and
  filters.
- TU-14.3 Built from a small design system in `web/design/`. For Theos it may borrow the Theos
  UI tokens, but the cockpit stays project-neutral.

### P15 · Decision inbox and review queue

**TU-15 Deciding and reviewing**
- TU-15.1 The inbox shows open escalations in the §5.4 format, with the details collapsed.
  Answering one works from a phone screen.
- TU-15.2 The review queue sorts by risk, then age. *Looks good*, *flag* and *revert* work as in
  §7.3. A flag opens a PR with a follow-up story draft in the planning repo.
- TU-15.3 After a flag, the decider proposes a directive or check text. Jann can accept it (it
  becomes a planning-repo PR for him to merge) or dismiss it.

### P16 · Controls and command bar

**TU-16 Steering**
- TU-16.1 Pause and resume for everything, a project or a component take effect before the next
  run starts. Running runs finish or are cancelled, as Jann chooses.
- TU-16.2 Reprioritise, re-run and cancel work on a story.
- TU-16.3 A free-text command returns the decider's proposed actions. Nothing runs before Jann
  confirms. Confirmed actions that change direction leave a record in git.

### P17 · Spend

**TU-17 Spend overview**
- TU-17.1 Tokens and API-equivalent cost per run, as the CLI reports them, aggregated by story,
  epic, component, project and day.
- TU-17.2 Usage against the budget windows, with rate-limit events and pauses on a timeline.
- TU-17.3 Cost per merged story over time, per component.

### P18 · Onboarding toolkit

**TU-18 New projects**
- TU-18.1 `loom project init` creates a planning repo skeleton (§2.1) with templates for the
  vision and directives.
- TU-18.2 `loom component add` registers a repo, runs the audit, and lists what is missing for
  the contract, with a draft compliance epic.
- TU-18.3 `loom import confluence` (space, page tree) writes Markdown with its source page id
  and date in front matter. `loom import jira` (project) writes one archive file per issue.
  Both are read-only on the source.
- TU-18.4 A written onboarding guide (`docs/onboarding.md`) takes a new project from nothing to
  its first merged story.

### P19 · Deployment on the home server

**TU-19 Running for real**
- TU-19.1 A container image of Loom and a compose file for the dedicated VM of T4 start it with
  its volumes, secrets and the worker image.
- TU-19.2 Nightly `pg_dump` of Loom's database and a copy of its config, with a tested restore.
  Backups of the database VM itself stay Jann's existing routine.
- TU-19.3 Loom updates itself only when Jann triggers it, never by itself.
- TU-19.4 A health page and a push notification when Loom itself is down or can't reach
  GitHub or Claude.

### P20 · Board, story and epic pages

**TU-20 Board and navigation**
- TU-20.1 The board shows every epic and story of the chosen project in the columns of §7 item
  0, with swimlanes by component or epic and the filters of §7. It updates live from the SSE
  stream.
- TU-20.2 Blocked cards carry a flag and keep their column. Cards with a waiting decision carry
  a marker that opens that decision.
- TU-20.3 Dragging a card to *Ready*, *Paused* or *Dropped*, or reordering within a column,
  commits only that front-matter field under Jann's identity with the `Via: loom-cockpit`
  trailer. Every other drag is refused with the reason (tested for each column pair).
- TU-20.4 The story page shows the summary, the ACs verbatim, dependencies and the tabs of §7
  item 0. *Activity* lists every run of the story with its questions, answers and review
  findings, and links each entry to its full log.
- TU-20.5 The epic page shows summary, epic ACs verbatim, its stories as a mini board, progress
  and open decisions.
- TU-20.6 Summaries are stored with their source hash and regenerated when the hash changes. A
  stale summary is never shown as current. No run's input contains a summary (verified from the
  run records).
- TU-20.7 A summary is at most 60 words, plain words, and passes the plain-words check of
  TU-12.4.

---

## 10. Onboarding Theos (work outside the build)

Done by Jann with Claude in planning sessions. O0 is stage 1 itself; O1–O6 and O9 come once the first version works; O7 and O8 belong to stage 3 (§9). Each item lands in the planning
repo or a component repo.

| Item | What |
|---|---|
| O0 Trial: building Loom | Stage 1 of §9 is the trial. Each phase of it runs by hand through the loop of §3.1, with this umbrella as the plan, so no BMad planning is needed for Loom: (1) a refinement session writes the phase spec and plan, with Jann answering its questions in the decider's place; (2) a **fresh** session implements from them; (3) fresh reviewer sessions loop until one is clean; (4) the PR merges on green. A project skill in Loom's own repo (`.claude/skills/loom-phase/`) drives these steps, the way `theos-phase` does in the editor. Per phase, record tokens and cost per step, wall time, review rounds and findings, questions asked, and Jann's verdict on the code (would he have merged it as is?). Jann's answers become Loom's first decision records and seed its directives. |
| O1 Planning repo (created in stage 1) | Create `FPGSchiba/theos-planning` with `loom project init`. Components: `engine` (Theos-Engine), `editor` (theos-editor), `game` (tower-of-theos) and `loom` (theos-loom, which runs through the same loop). Created early, on 27 Nov 2026, so P01 can test against real files. |
| O2 Vision | Write `vision.md` from the project instructions, the Strategic Technical Evaluation v2 and the Tower of Theos canon summary. Jann approves it. |
| O3 Directives | Project-wide: quality over speed; the canon authority Lore → UX → Tech; local-only LLM; no creative content by agents. Engine: Option B gated plan, single microkernel, ECS on `hecs`, `wgpu`, LLM agentic layer first. Editor: theos-editor's engineering directives and global constraints. Game: logic only, builds only from approved design, Theos UI design system and Visual Canon for UI. |
| O4 Canon to git | `loom import confluence` for space TOT into `canon/`: Core Lore, Shared Formalism, Core UX, Interfaces, feature designs, Visual Canon, engine specs, Decision pages. Confluence becomes read-only. A later sync can publish copies back once initiatives finish. |
| O5 Jira archive | `loom import jira` for THEOS into `archive/jira/`. Useful acceptance criteria are lifted into new stories during planning. **Jann deletes the Jira project himself** once the archive is checked. |
| O6 Editor specs | theos-editor's umbrella and its specs become the editor component's initiative. Its slices become epics, and its remaining phases become stories (a phase is already one branch and one PR), and their CE ACs become story ACs with their ids kept, so `ac-trace` and existing tags keep working. Its `CLAUDE.md` is rewritten for the unattended overlay: brainstorming and the phase spec stay, but the decider approves them where Jann did. That's a protected change, so it waits for Jann. The repo keeps a pointer. Its gate stays as is and gains `loom-contract` and `loom-review`. Its label-based `approval` check is replaced by `loom-approval`. The protected paths are narrowed so plans and phase specs built from an approved story no longer wait for Jann (they were approved at planning). The gate, agent instructions and ACs stay protected. |
| O7 Engine rule change (stage 3) | Agents write engine code. Jann reviews after merge. The project instructions and `GOAL.md` habit change accordingly. Theos-Engine gets a gate to the contract (a compliance epic, waits for Jann). |
| O8 Tower of Theos (stage 3) | Forbidden paths for agents: lore, design records, world content, art and binaries references (exact globs set at onboarding). Agents work in the game code and tools folders only. |
| O9 First epics | One small epic per component, planned with the kit, as the first real run. |

---

## 11. Later (not planned)

Webhooks instead of polling. A Confluence publisher for finished initiatives. Several Claude
accounts or an API key per project. A GPU worker pool (Jann's PC) for stories that need a GPU,
such as renderer tests and the local LLM. Learning the decider threshold from Jann's
agree/disagree in the review queue. Cost forecasts per
epic before it starts.

---

## 12. Calls made while writing (for Jann to check)

Decided by Claude, not by Jann. Each can be overruled with a one-line change.

- **Budgets** are caps on Loom's own token spend per window, plus backing off on rate limits.
  The subscription's remaining usage isn't visible to Loom (§3.3).
- **The refiner also writes the plan.** The plan decides which tests prove the ACs, so it
  belongs with the design, and a fresh implementer building from it tests whether it's complete
  (§3.1, §4).
- **Each brainstorming question is its own decider run**, and an escalation ends the refiner run
  with a checkpoint instead of leaving it waiting (§3.1).
- **Workers get planning files copied in, read-only**, and never hold a planning-repo token. The
  slicer's output becomes a PR through Loom's service (§3.2).
- **Sliced stories are Loom's** until Jann edits one (§3.2).
- **Editor phases become stories, and its slices become epics.** Design and plan paths are set
  per component (§4, O6).
- **Refinement starts after the story's dependencies are merged**, so the design is based on the
  code as it really is (§3.1).
- **Design approval is capped at 3 rounds**, then the decider escalates (§3.1).
- **`slicing = auto` is the default** per component, and `review` is the alternative (§3.1).
- **One final review loop**, Loom's review service. The worker keeps superpowers' per-task
  reviews but doesn't run its own whole-branch loop (§4).
- **Planning repo writes**: Loom pushes straight to `main` only in `_state/` and `decisions/`.
  Story drafts and proposed directives go as PRs that Jann merges (§3.2).
- **The contract check runs in Loom's service**, not in component CI, so component repos hold
  no token to the planning repo (§6.1).
- **A dedicated VM** on the home server rather than an LXC (T4).
- **GPU-bound tests** are tagged, left out of the required checks and run on a schedule. `wgpu`
  gets a software-adapter path in the required checks (R12).
- **BMad fork** from v6.12.0, pulling from `next` only deliberately (§4).
- **PostgreSQL with `sqlx`** (Jann's call), with its credentials kept out of worker containers (T4b); **SSE** for live updates, **ntfy** for push, **hybrid local search**
  for retrieval (§5.3, §7, T1).
- **ACs are shown verbatim, never summarised.** Only the prose of a story or epic gets a
  summary, because the ACs are what binds the work (§7).
- **Summaries live only in Loom's database**, keyed by the source file's hash, and no run ever
  reads them. A wrong summary can mislead Jann but never an agent (§7).
- **The board can change only Jann's fields** (ready, paused, dropped, priority). Those changes
  are committed as his own edits (§3.2).
- **Blocked is a flag, not a column**, so a stuck card stays where its work is (§7).
- **Jann answers the refiner's questions himself during the trial**, in the decider's place, so
  his answers seed the first decision records and directives (O0).
- **Staged build.** Loom builds itself first, then the editor joins. The engine and Tower of
  Theos come only in stage 3: the engine's GPU code is the hardest to check automatically, and
  Tower of Theos' game logic waits on designs that mostly don't exist yet (§9).
- **Approval of protected changes** happens in the cockpit, and the App posts `loom-approval`.
  Labels are only a hint (R5).
