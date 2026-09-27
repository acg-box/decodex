# Restore native settings observation qualification

The inherited coordinator tests covered saved model, permission and plugin facts,
private-data projection, no-dispatch behavior and database reopen. The current
coordinator persists transport-owned observations through three existing owners.
Direct calls to the old raw-response projection helpers no longer exercise that
path. Restore the common behavior through one real in-memory native transport.

The new fixture sends each notification through `AppServerClient`, waits for its
event and then invokes the coordinator. It keeps the same connection across all
changes. It verifies foreign-thread exclusion, all three projections, A-B-A
transitions, invalidation by incomplete facts, durable records after reopen and
absence of pending or wake events. A read of the native socket also verifies that
observation does not issue a request. Private collaboration instructions are
excluded from all saved projection payloads.

A second fixture gives the coordinator a stale queued payload after the transport
has published newer settings. It cannot replace the saved current facts. The
inherited pure hydration-validation test is restored with its complete original
assertions for exact thread, model, unknown effort strings, explicit null and
missing or malformed fields.

Repeated reads of the same transport revision keep the same event IDs. A new wire
publication has a new revision even when the values match. That deliberately
invalidates old mutation reviews. The inherited raw-value duplicate assertion is
not an equivalent identity check for the current transport revision owner. The
new test explicitly verifies both cases, plus A-B-A preservation. No production
behavior is changed to make these assertions pass.

This restores common projection qualification, not the complete inherited test
file. Its resumed-response and pending-permission receipt scenarios still need
complete consumer mapping. Keep that file row open. No schema, installed native
binary, live account or maintenance automation changes. These fixtures do not
establish signed desktop acceptance.

All three restored qualification tests pass. Strict runtime Clippy passes with
all features and targets. Logs: `/tmp/decodex-native-settings-observations-final.log`
and `/tmp/decodex-native-settings-observations-clippy.log`.
