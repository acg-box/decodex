# Native configuration diagnostics

This batch extends the existing Chief warning path. It does not add another
configuration owner or change permission, write-receipt or retry decisions.

## Native references

The fixed cutoff is `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Upstream `935ac7710da6` refreshes global instructions at model-request boundaries
and can emit a thread warning when a read fails. Upstream `4fa7e82274bd` preserves
configuration error causes, including file locations and parse errors, when a
reviewer setting cannot be saved. The installed experimental schema includes
`WarningNotification` with a public message and optional thread identity.

## Chief consumers

Startup buffering retains bounded `configWarning` and `warning` messages in
arrival order. The existing host persists process-wide warnings on the current
root and thread warnings on their exact owned task. Old or foreign process
observations do not become current history. Duplicate text from startup and
runtime notifications is stored once for the same task and generation. The
existing cap remains; overflow produces one bounded notice.

Hook, connected-account, saved-connection and connector-exposure settings retain
public native RPC error messages. The projection removes private error data,
filters credential material and rejects oversized or malformed messages. It uses
an explicit operation label rather than Debug output. Source checks and the
current process owner apply before persistence.

Warnings appear as execution notices and survive database reopen. They do not
wake the model, answer approvals, terminate a turn, convert an unknown write to a
failure or authorize a retry. The original shared configuration receipts remain
in force. This batch requires no database or local protocol version change.

## Validation boundaries

Unit and database fixtures cover redaction, field selection, ownership, duplicate
suppression, bounded storage and no wake. The startup subprocess fixture carries
both notification forms through initialization and retained handoff. The native
opt-in fixture uses an isolated home and loopback Responses server; it causes an
unreadable global AGENTS.md and checks retained history after reopening the store.
The setting-error fixture checks that an unconfirmed write stays unconfirmed while
its public parse cause becomes readable.

The ordinary-conversation warning projection in the inherited snapshot is a
separate outstanding consumer. This Chief delivery does not claim that all
ordinary warning or partial-output behavior is integrated.

Final batch validation: database118 and runtime563 tests passed;41 opt-in tests
were skipped. The new installed-native warning test passed separately. Strict
database/runtime Clippy passed across all targets and features.
