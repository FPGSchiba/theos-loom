# UI reference

The fifteen screens of the Loom cockpit, exported from the design canvas *Loom cockpit — UI design*
on 27 Nov 2026 at 1920 × 1080 (a 16:9 monitor). Each screen is fixed to the window; only panels
inside it scroll.

- **`screens/NN-<name>.html` is the reference.** Each file is plain HTML that uses only
  `web/design/theos-ui/tokens.css`, `web/design/theos-ui/components/bundle.css` and a short block of
  page styles. Open it in a browser from a clone, or read it: the class names (`tu-*`), the
  structure and the exact texts are what the cockpit should reproduce with React components.
- **`screens/NN-<name>.png` is a preview** rendered from that HTML, so GitHub and reviewers can see
  the screen without a browser. If the two ever disagree, the HTML wins; regenerate the PNG.
- **Themes.** Every screen is drawn in Grid Noir, the default. Set `data-theme="grid-light"` on a
  screen's first element to see Grid Light; `screens/01-board-light.png` shows the board that way.
  The cockpit offers both and remembers the choice.
- **States.** The canvas screens are clickable; the exports show their first state. Screens 03–09
  are the same story page in different states (tab, open agent, full log), so all its tabs are on
  record.

Sample content is illustrative: story titles, numbers, ACs and decisions are made up to look like a
working week. Where it disagrees with `docs/specs/00-umbrella.md`, the umbrella wins.

| Screen | Name | Phase |
|---|---|---|
| `01-board` | Board — lanes by component, filters, command bar entry | P20 |
| `02-epic` | Epic page — stories as a mini board, order, epic ACs | P20 |
| `03-story` | Story page — refining, a question sent to Jann | P20 |
| `04-story-agents` | Story page — in review, runs with their agents, one agent's actions open | P20 |
| `05-story-log` | Story page — full action log of every agent | P20 |
| `06-story-decisions` | Story page — decisions tab | P20 |
| `07-story-pr` | Story page — pull request, checks, review rounds | P20 |
| `08-story-spend` | Story page — spend by step and by agent | P20 |
| `09-story-source` | Story page — story file, design and plan | P20 |
| `10-inbox` | Decision inbox — an open escalation in the umbrella's format | P15 |
| `11-activity` | Activity log — runs, one run's agents, actions, rulings, checks | P14 |
| `12-review` | Review queue — merged changes by risk; looks good, flag, revert | P15 |
| `13-spend` | Spend — totals, per day, per merged story, breakdown, windows | P17 |
| `14-controls` | Controls — pause, queue order, budget windows, threshold, notifications | P16 |
| `15-command` | Command bar — the decider's proposal before Jann confirms | P16 |

## What the screens settle

- **Shell.** Title bar, a rail with Board, Activity, Inbox, Review, Spend and Controls, and a
  status bar with the decider's threshold, the week budget and the rate-limit state.
- **Colour means the same as in Theos UI**, mapped onto Loom: amber = Jann needs to act; magenta
  (always with the `AI` marker) = the decider decided, or the text is an AI summary or
  recommendation; cyan = running; green = done or merged; red = blocked or failed; yellow = paused,
  budget and rate-limit warnings. Violet is unused.
- **Agents are one level below their run.** Every step shows its session and the agents it
  started (implementer and task reviewer per task, reviewer and fix agent per round), each with
  model, worktree or "read only", time, tokens and last action. Every action of every agent stays
  visible: per agent in the actions panel, and for the whole story in the full log.
- **Escalations never show internal numbering.** Titles and texts name things in plain words; file
  paths appear only in the collapsed "Details" part.
- **Acceptance criteria are always verbatim.** Summaries carry the `AI` marker and the note that
  they are never used as input.
- **Charts use one neutral ink**, not a series palette, because Theos UI keeps its colours for
  meanings. Bars are square like the rest of the chrome.

## Open point

TU-14.3 says the cockpit may borrow the Theos UI tokens "but stays project-neutral". These screens
use Theos UI as a whole (tokens and component CSS), as Jann decided on 27 Nov 2026. The chrome itself
shows only Loom and the project's name as data, so another project would get the same cockpit. Whether
TU-14.3 should be reworded is Jann's call; the umbrella is a protected path and is not changed here.

## The design system

`web/design/theos-ui/` is a copy of the Theos UI design system (tokens, component CSS, the theme
helper script and the fonts under the SIL Open Font License) as of 27 Nov 2026, including the Grid
Light theme. Its README is the system's own and also covers the Theos Editor. Update the copy as a
whole when Theos UI changes; do not edit it here.
