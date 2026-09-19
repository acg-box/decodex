# Codex capability reference

Reference checkout: `target/upstream-codex` (ignored, read-only reference use).
Official repository: https://github.com/openai/codex
Reviewed main commit: fd346b8dbaa24573a0244bc917811849d27c4cf4
Installed binary: codex-cli 0.154.0-alpha.6.2

## Current findings

- `codex-rs/tui/src/token_usage.rs` distinguishes the latest active context from
  cumulative session consumption. Context uses `last.totalTokens`, with capacity
  from `modelContextWindow`. Decodex shows raw reported occupancy; it does not copy
  the TUI's presentation-specific reserved-baseline adjustment.
- `codex-rs/app-server/src/bespoke_event_handling.rs` forwards `TokenCount` snapshots
  as `thread/tokenUsage/updated`. A snapshot is not an incremental debit. Repeated
  notifications must not add the previous request's usage again.
- `codex-rs/app-server/src/request_processors/token_usage_replay.rs` replays the
  persisted usage snapshot when a client attaches to an existing thread. Decodex
  accepts one expected historical turn from the exact resume response, preserves
  newer live observations, and restores current context and a turn baseline when
  the installed provider emits that notification. It does not require replay to
  be present to continue a conversation.
- The installed binary's generated schema exposes `threadId`, `turnId`, cumulative
  `total`, latest `last`, and optional `modelContextWindow`. It does not expose a
  direct completed-turn usage field. Decodex therefore snapshots the cumulative
  counters at turn admission and seals their delta into the completed-turn record.
- A fresh provider thread starts with known zero consumption. Unknown historical
  baselines, counter resets, and completion recovered without final usage remain
  unreported. Resuming a thread changed by another client invalidates stale counts.

## Product result

Show work duration and input/output token counts once below each completed reply.
Show only current context occupancy near the composer. Preserve the reported
numbers, do not estimate them from text length or model names, and never use
cumulative token consumption as context occupancy.

Migration 20 adds nullable turn baselines to the existing service-owned usage
snapshot. Completed counts are retained in the exact immutable completion event.
Protocol 2.19 exposes optional turn usage. Earlier records remain compatible with
missing usage; no historical values are fabricated.

## Recurring review

The existing Codex Upstream Maintainer is active on its existing six-hour schedule.
Its prompt now includes useful product capabilities as well as compatibility,
source/test evidence, installed-version checks, and quiet unchanged runs. The
worktree setup script is disabled for this automation. Other paused automations
were not activated. The checked-in maintainer prompt carries the same review focus.

## Validation and preview

Database, protocol, GPUI, and runtime suites passed for the usage change; the final
runtime suite includes 302 passing unit tests plus integration tests. Strict Clippy,
repository formatting, architecture checks, and signed staging passed. Inspected
`target/visual-tests/chief-turn-usage.png`: per-turn metrics are below the reply and
only context remains below the composer. This is a native capture fixture, not a
fabricated result in the real Chief history. Reopened one signed preview process.
The existing external Codex writer conflict still prevents a live model round trip;
the request to release that exact Chief task remains unanswered.
