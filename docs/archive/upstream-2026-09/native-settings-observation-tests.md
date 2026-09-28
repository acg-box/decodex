> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

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

## Complete inherited test mapping

| Inherited test | Current qualification |
| --- | --- |
| Plugin publications survive reopen without waking or exposing settings | The combined wire test checks the exact plugin projection, empty exclusions, null invalidation, exact source scope, saved records and no native writes or wake events. |
| Hydration accepts native changes but rejects foreign or malformed replies | The original pure validator test retains every assertion. |
| Resumed settings require an exact thread and complete reply | The resume fixture crosses the real RPC boundary, rejects a foreign response, accepts known null effort and tier, and invalidates a later response with a missing effort field. |
| Native settings retain transitions without waking or exposing instructions | The combined test verifies model projection, private-field exclusion, A-B-A events, source-scoped reads and writes, invalidation and reopen. |
| Permission observations are separate, durable and invalidated by incomplete facts | The same fixture verifies a distinct permission event, exact profile and no model/plugin field substitution, then tests incomplete metadata and reopen. |
| Queued or historical permission facts cannot confirm selection | A queued raw payload and an actual `thread/read` reply both retain the queued receipt and dispatch fence. A subsequent wire publication can confirm; that receipt survives reopen. |

The complete inherited test file was compared with these current fixtures. Close
only its file row. The shared production observer and other writer files retain
their separate open reviews. No schema, installed native binary, live account or
maintenance automation changes. These fixtures do not establish signed desktop
acceptance.

All five qualification tests pass. Strict runtime Clippy passes with all
features and targets. Logs: `/tmp/decodex-settings-response-final.log` and
`/tmp/decodex-settings-response-clippy.log`.
