# loom-store

Loom's library for PostgreSQL through `sqlx`, with migrations by `sqlx migrate`.

It stores:

- the raw stdout and stderr of every run, as chunks (`run_stream_chunks`), kept for 30 days
  (`prune_raw_streams`, T7);
- the downloaded price list (`price_lists`, T14).

It implements the storage traits of `loom_core::run`.

## Migrations

They live in `migrations/`, are applied by `loom_store::migrate`, and are never edited after they
merged. A phase adds the next file (P02 adds `0002_...` after `0001_run_runtime.sql`).

## Tests

Tests use a throwaway database made by `loom_store::testing::TestDb` (T4b). Start a throwaway
PostgreSQL and point the tests at it:

```sh
docker run -d --rm --name loom-pg -p 5432:5432 -e POSTGRES_HOST_AUTH_METHOD=trust postgres:18
export LOOM_TEST_DATABASE_URL=postgres://postgres@localhost:5432/postgres
```

Without the variable the tests fail; they never skip.
