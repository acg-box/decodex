# Native execution activity

## Behavior

Chief and worker conversations show native execution receipts. The current action appears while a turn runs. Consecutive steps form a compact disclosure in the continuous conversation. Click it, or press Enter or Space when focused, to show steps, tool names, outcomes, and provider-reported durations. Opening uses the existing disclosure animation.

Supported items: reasoning (status only), command execution, parsed file reads and searches, file changes, MCP and dynamic tools, web search, agent coordination, context compaction, image viewing, and image generation. Existing approval and user-input panels remain authoritative.

The UI does not show private reasoning, raw arguments, command output, or provider frames. A missing completion remains unconfirmed after the turn ends or the connection becomes uncertain. Failure is visible in the collapsed summary. This change captures new native notifications; it does not fabricate past tool history.

## Native authority

- Official `openai/codex` reference checkout: `fd346b8dbaa24573a0244bc917811849d27c4cf4`.
- Fetched upstream main: `fc2ea82e7eff22c618a56db29c68a6b1967cba7d` (reference checkout unchanged).
- Installed Codex: `0.154.0-alpha.6.2`.
- Installed experimental schema generated under `target/codex-voice-schema` confirms `item/started`, `item/completed`, and `contextCompaction`. Source authority: `codex-rs/app-server-protocol/src/protocol/v2/item.rs`.

## Storage and protocol

Protocol 2.22 adds an optional activity field to a history entry. Observations use the existing immutable inbox, already resolved. They do not wake an agent or change work acceptance. Each receipt must match a currently running thread and turn. Identity plus stage makes repeated delivery idempotent. The history query suppresses a start after its completion exists; the UI also suppresses starts in older cached pages.

No schema migration is required. Each turn stores at most 256 receipts, each at most 4 KiB. Page budgeting includes activity payloads. Unsupported item types are ignored. Long steps can remain unconfirmed if the receipt bound is reached; a terminal turn alone does not prove that each tool succeeded.

## Validation

- Store test: exact thread and turn, duplicate delivery, start/completion projection, no wake, no work-state mutation, restart persistence.
- Runtime tests: native event handling, compaction, failed commands, unsupported messages, exclusion of raw arguments/results.
- Visual capture pages `activity` and `activity-collapsed` are explicit test fixtures. They never run in the normal application or write to user conversations.

Verified with the database, protocol, runtime, and GPUI test suites; strict Clippy; the 16 architecture checks; and expanded/collapsed offscreen renders. The additional history projection test confirms that internal disposition prose does not leak into activity rows. No live provider request was sent as a test.
