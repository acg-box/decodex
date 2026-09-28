> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Native realtime history and voice-tail qualification

Restore the complete inherited realtime-history fixture through the current
shared Responses backend. Its response IDs, text, usage, WebSocket events and
original assertions remain unchanged.

The installed Codex CLI 0.158.0-alpha.2 test passes. Assistant and user speech
precede typed input in the native timeline. The promoted shared artifact resolves
through the Chief timeline reader. The ended voice boundary occurs once, and
completed realtime events match persisted entries. After the native process and
product store reopen, the full timeline remains equal. Recovery sends no new
model request or input; the complete case uses two synthetic Responses requests.

## Transcript persistence reconciliation

Restore four inherited tests through the real coordinator event path, admitted
ownership fixture, voice start and SQLite store. They cover alternating user and
assistant deltas, Unicode limits, failed final writes, repeated transport closure,
reopened history, failed signaling state and no native replay. Transport loss
retains the open call because it does not prove process death.

The old oversized-final test expected a prefix. The retained production owner
already keeps the most recent UTF-8 suffix and marks truncated text partial, as
specified in [voice transcript recovery](voice-transcript-recovery.md). Update
that expectation and assert saved finality explicitly. Do not restore the older
prefix behavior. All other inherited assertions remain.

Read the complete 206-line diff of `chief/voice.rs`. Retain its single save owner:
advance sequence and clear text only after storage succeeds; retain corrected
final text and finality after failure; save pending final text before another
part; preserve received tails on precaution stop and disconnect. Retain native
resume parameters, the explicit realtime feature override on voice start, and
native source identity checks. No production source changes are needed.

## Validation and limits

- Installed-native realtime history fixture: pass.
- Four restored coordinator voice-tail tests: pass.
- Seven current voice actor and persistence tests: pass.
- Strict runtime Clippy, all features and targets: pass.

Fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582` includes native realtime
timeline and promoted-artifact tests. Relevant fixture and ordering assertions
were read; upstream tests were not executed here. The restored native fixture
uses a local text WebSocket transport. It does not qualify microphone capture,
audio media, WebRTC connectivity, real providers or signed desktop interaction.

Close the native realtime fixture, coordinator tail fixture and complete voice
actor dispositions. Shared test parents, larger timeline/UI files and final voice
acceptance remain open. No installed configuration or automation is changed.
