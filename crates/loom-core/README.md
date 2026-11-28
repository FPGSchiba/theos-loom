# loom-core

Loom's library for the domain and its state machines.

- `run`: the domain types of a run (`RunId`, `RunEvent`, `StopCause`, `Usage`, `CapKind`) and the
  `RawStreamStore` and `PriceListStore` traits the run runtime stores through. It has no
  database or runtime dependency.
