# Native Code Mode and compaction qualification

Restore three complete inherited Rust fixture files and the delayed MCP Python
server. Keep their original assertions. Adapt their backend calls to the current
shared Responses fixture. One implementation still parses requests and emits
responses; its new usage callback supports token changes between requests.
Existing fixed-usage callers retain the same wrapper and wire values.

## Current native evidence

The explicit installed Codex CLI 0.158.0-alpha.2 and its packaged helper pass:

- Yielded cell, second cell and wait: outputs and notifications retain their
  originating call IDs. Cleared timers and timers from completed cells emit no
  late output. Exactly four model requests occur.
- Delayed MCP: two nested calls retain the original thread, session, window and
  code item metadata. Both completed items remain under the original turn before
  and after native process restart, with no additional model request.
- Automatic compaction: started and completed events share the same native
  identity. Later input and cold continuation retain the exact checkpoint and
  original user request; no compaction trigger leaks into continuation input.
- Failed compaction: the incoming user prompt precedes the error and occurs once
  in persisted history after restart. Cold reading does not replay input.
- Code Mode metadata: a real MCP call, tool discovery without invocation and an
  error before invocation each retain the exact completed inventory in the next
  model request and compaction request. Empty inventories are explicit. All three
  cases continue from the saved checkpoint after native restart.

The five restored tests pass. Existing native history and audio tests also pass
through the shared fixture, and strict all-feature, all-target runtime Clippy
passes. Tests use disposable homes, synthetic local responses and a local MCP
server. They use no real account credentials or external inference.

## Authority and limits

Fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582` retains cell-bound
execution delegates and abort-on-drop timer tasks. Its app-server compaction test
checks exact started/completed identity and continuation from an encrypted
checkpoint. These source checks support native ownership; upstream tests were
read, not executed here. Decodex does not add a JavaScript runtime, metadata
recorder, retry policy or context compactor.

Close only the three complete native test-file dispositions and the exact Python
fixture. The shared native-test parent, the historical Code Mode review, Guardian
cross-turn policy, still-running nested calls during compaction, failed metadata
rebudgeting and signed desktop acceptance remain separate open items. No normal
profile or automation is enabled by these opt-in tests.
