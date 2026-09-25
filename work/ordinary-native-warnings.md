# Ordinary native warnings

Classification: core compatibility for existing ordinary conversations. This
consumer retains native diagnostics; it adds no configuration or execution owner.

The fixed upstream commit is `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Its `protocol/v2/notification.rs` defines the public warning message and optional
thread identity. `protocol/v2/config.rs` defines configuration summary and details.
The consumer uses the existing bounded, credential-filtered projection and ignores
private fields. Installed-native qualification uses Codex 0.155.0-alpha.16.4.

The process gateway retains startup, thread-start and thread-resume notices until
an accepted turn has a durable history owner. Notices are not execution evidence:
a warning received during idle resume does not make that resume ambiguous.
Runtime storage rejects another thread's warning and stores each distinct message
once per provider attempt, with a limit of 64 messages. Status items belong to the
logical user turn. They do not complete a turn or request another model response.

The local protocol is 2.76. `ConversationHistoryChanged` tells the client to reload
stored history. It does not carry assistant text, change command acceptance, or
claim turn completion. The history pager refreshes only the affected conversation;
closed conversation history is marked stale for its next open.

Qualification covers the subprocess warning decoder and idle resume. The native
fixture uses synthetic credentials and a loopback Responses server. It makes an
existing global AGENTS.md unreadable between explicit turns, checks the public
history refresh and Status projection, and reads it again after database reopen.
It also preserves the existing exact model-request count across restart. This is
not an external-provider or interactive desktop acceptance claim.

Validation: runtime 565 passed with 41 opt-in tests skipped; protocol 134 unit
and 6 integration tests passed; GPUI 461 passed with 5 opt-in tests skipped.
The installed-native fixture passed separately (one persisted warning, four
explicit model requests across restart). Strict Clippy passed for protocol,
runtime and GPUI across all targets and features.
