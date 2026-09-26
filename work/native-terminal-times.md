# Native terminal time retention

Terminal event projection dropped `startedAt` and `completedAt`. It also retained
an unchecked `durationMs` value and marked a retained duration as omitted.
Restore the inherited bounded projection: retain nonnegative integer native time
fields and use null for missing or invalid values. Do not synthesize timestamps
from local recovery time. Mark details omitted when a source field is changed or
removed.

Fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582` defines these
optional signed 64-bit fields in
`app-server-protocol/src/protocol/v2/thread_data.rs`. Installed Codex
0.158.0-alpha.2 generates the same fields in `TurnCompletedNotification`: start
and completion are Unix seconds; duration is milliseconds.

The restored unit regression reproduced a missing native start timestamp before
the fix. It covers complete values, old events without timing, negative values,
oversized strings and invalid objects. The paginated recovery regression checks
that original native times reach the durable completion event without starting
another turn or hydrating full thread history. Existing large-result and
provider-duration checks remain.

This batch changes retained evidence only. It does not introduce a new usage
store or estimate missing timing values.

The full inherited result-integrity test file is retained. The large-result
regression now covers both native messages with an item ID and legacy messages
without an ID. Both cases preserve a valid text prefix and keep the completion
event within 65,536 bytes without duplicating terminal items. Both cases pass;
this closes a coverage omission and does not identify a production failure.
