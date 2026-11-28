# 0023 · Directives are named; a marked contradiction is an error

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P01 refinement

## Decision

- A directive is a numbered list item with a bold name (`1. **Quality over speed.** …`) and is
  identified by that name, not its number, so reordering a list breaks nothing. Names are unique
  within their file.
- A component directive that knowingly contradicts a project directive carries one line naming
  it: `Contradicts: Quality over speed`.
- `loom plan check` reports every such mark as an error naming both directives and both files,
  and a mark that names no existing project directive as a broken reference. A contradiction
  stays visible and blocks until Jann changes one of the two, or makes it a real exception in the
  project file.
- Resolving by layer: Loom hands a run the project directives first, then the component's
  engineering and design directives, each labelled with where it came from.

## Options considered

- **A contradiction as an allowed override that is only reported**: component directives are
  meant to add to the project's and never contradict them (umbrella §2), so an override would
  quietly change a project-wide rule for one component.
