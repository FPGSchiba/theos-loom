# Stream fixtures

One JSON object per line, as `claude -p --output-format stream-json --verbose` writes them
(JSONL has no comments, so this file says what each one is).

| File | Origin |
|---|---|
| `normal_run.jsonl` | **Real recording**, 28 Nov 2026, Claude Code CLI 2.1.322, on a signed-in plan. |
| `auth_failure.jsonl` | **Real recording**, same day and version, with an invalid token. |
| `rate_limit_rejected.jsonl` | **Synthetic**: a `rate_limit_event` whose status is `rejected`. Not triggerable on demand. |
| `result_429.jsonl` | **Synthetic**: an error `result` with `api_error_status` 429. Not triggerable on demand. |

Recorded from an empty directory with
`claude -p "Reply with hi." --output-format stream-json --verbose --model haiku --max-turns 1`.
The auth failure used the same command with `CLAUDE_CODE_OAUTH_TOKEN=invalid` alone; the CLI
retried twice (`api_retry` events) and ended with a 401 `result`. No other variable was needed.
The token value is the literal word `invalid`, never a real token.

Scrubbing of the two real recordings: `session_id`, `uuid`, `request_id` and message ids (`msg_...`) replaced by
fixed fake values, and `credential_expires_at` by a fixed fake timestamp; the user name, home and working directories replaced by `/work` and
`/home/user`; the `init` event's machine-specific lists (`tools`, `slash_commands`, `skills`,
`plugins`, `agents`, `mcp_servers`) shortened to a few fixed entries and its socket and
PowerShell paths removed; the recording machine's hook events (`hook_started`,
`hook_response`) dropped. Everything else is as the CLI wrote it, re-serialised compactly.
