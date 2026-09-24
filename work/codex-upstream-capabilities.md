# Codex capability reference

## Dynamic reasoning effort — 2026-09-24

Reference: openai/codex 595cc91e8cbb1c2ca822d0311dcf12709410c582,
`codex-rs/protocol/src/openai_models.rs`. Native ReasoningEffort retains custom
model-defined strings. Installed codex-cli 0.155.0-alpha.16.3 supports this path.

Protocol 2.50 retains bounded custom effort values across model discovery,
explicit message settings, desktop controls, and cold draft storage. Unknown
values display their exact name instead of High. Legacy `x_high` saved values
still map to native `xhigh`. Both protocol and native request adapters accept
up to 128 UTF-8 bytes and reject empty values and control characters.

Validation: 128 Codex adapter tests, 109 protocol tests, 485 runtime tests, and
310 desktop tests passed. Strict Clippy passed for all four affected packages.
An isolated installed-native test reads the model catalog through Decodex,
starts Chief with the advertised custom effort, and checks the exact outbound
Responses value at a loopback backend. It completed with one model request and
no real credentials. This proves the custom-effort path, not all model capability
or workspace-routing integration.

This batch is rebased on workspace PR #1393 (37a88d008). It retains the native
agent hierarchy, live output subscription, weather cards, interruption controls,
and the previously delivered draft and receipt recovery. Small owner extractions,
explicit imports, and public field documentation repair strict validation failures
introduced by that baseline; no lint checks are disabled.

## Earlier usage audit

The following reference and acceptance notes describe the earlier usage change.
They are not the current manual catch-up completion status.

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

The Codex Upstream Maintainer remains paused during the manual catch-up.
Its configured daily time is 20:05 UTC, or 04:05 Beijing time, without seasonal
changes. Resume only after the manual catch-up meets its completion requirements.

## Validation and preview

Database, protocol, GPUI, and runtime suites passed for the usage change; the final
runtime suite includes 302 passing unit tests plus integration tests. Strict Clippy,
repository formatting, architecture checks, and signed staging passed. Inspected
`target/visual-tests/chief-turn-usage.png`: per-turn metrics are below the reply and
only context remains below the composer. This is a native capture fixture, not a
fabricated result in the real Chief history. Reopened one signed preview process.
The existing external Codex writer conflict still prevents a live model round trip;
the request to release that exact Chief task remains unanswered.
