# loom-core

Loom's library for the domain and its state machines.

So far it holds the lifecycle states (`src/state.rs`, umbrella §3.2), serialized in kebab-case.
There are no transitions yet; P02 adds them.

- `EpicState`: `intake`, `slicing`, `active`, `blocked`, `done`.
- `StoryState`: `queued`, `refining`, `refined`, `implementing`, `in-review`, `blocked`, `merged`.
