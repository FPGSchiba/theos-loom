# 0024 · Keys, criteria ids and cross-component prerequisites

Date: 28 Nov 2026 · Decided by: Jann · Surfaced by: P01 refinement

## Decision

- Every epic and story carries `key:` in its front matter: letters, a dash and a number
  (`ED-12`), unique in the project. The BMad PO's numeric `id` stays as she uses it.
- A criterion's id is the key plus its number: `ED-12.3` for a story's third criterion, `ED-4.R2`
  for an epic's requirement line R2. Tests carry these as `AC: <id>`. The PO keeps writing plain
  numbers in the files.
- Prerequisites in another component go in a Loom key, `needs: [EN-4, EN-7]`, by the key of an epic or a story. The PO's
  `after` keeps the prerequisites inside the initiative, in her forms. Loom's dependencies are
  both together; cycles and unknown keys are checked across both.

## Options considered

- **AC ids as the PO's local numbers** (1, 2, 3): not unique, so a test tag could not say which
  story it proves.
- **AC ids written by hand on every criterion**: works, but the PO would need a new criterion
  format.
- **Cross-component forms in `after`** (`engine:2.3`): the PO's tools report them as problems.

## Consequences

- The editor's existing ids (`CE-07.3`) map onto this form, so its tags keep working at
  onboarding (umbrella O6).
