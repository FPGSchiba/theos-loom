create table run_stream_chunks (
    run_id uuid not null,
    seq integer not null,
    stream text not null check (stream in ('stdout', 'stderr')),
    written_at timestamptz not null default now(),
    body bytea not null,
    primary key (run_id, seq)
);
create index run_stream_chunks_written_at on run_stream_chunks (written_at);
create table price_lists (
    fetched_at timestamptz primary key default now(),
    source_url text not null,
    body jsonb not null
);
