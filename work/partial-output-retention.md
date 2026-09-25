# Partial output retention

This change connects storage, native events, protocol and desktop consumers.

Classification: preserving existing assistant output is core correctness. Live
proposed-plan display is an optional presentation addition for user review.

The inherited snapshot contains storage, native event, history projection and
UI changes for unfinished assistant messages and proposed plans. Current main
before this batch stores active text, but filters it out after a terminal event;
a later turn can remove the old stream rows.

## Storage owner

`database/migrations` is the versioned schema authority. Migration 37 adds item
kind and completion state to `chief_live_output`. The existing application-owned
migration runner uses an immediate transaction and preserves applied names and
checksums across both supported historical lineages. No production database is
modified during this task. The upgrade test starts at version 36 with stored text,
checks exact row preservation and prior checksums, and verifies reopen parity.

`complete_chief_turn_with_event` copies unfinished assistant/plan text into resolved
inbox observations in the same transaction as the terminal receipt. The existing
32-item and 64 KiB bounds apply; the latter includes JSON escaping. These records
are display fallbacks, not results, pending work or authorization to retry. Current
long-poll revision notifications remain in force.

A correction to the inherited implementation is necessary: a late item-completed
notification must not delete the only saved fallback before native history can be
read. This batch retains it. The UI must suppress it only when it has the exact
matching native turn/item, without matching text or hiding a whole turn.

## Consumer behavior

Protocol 2.77 carries an optional native source triple on retained history entries
and a distinct live text kind. The projection budgets encoded source metadata and
removes coordinator disposition prose. Both native and saved-history views label
unfinished output and retain source text for the existing copy control.

The native timeline suppresses a fallback only for the same thread, turn, item and
kind with non-empty, untruncated native content. A terminal boundary alone is not
replacement evidence. Missing pages, another item with the same text, another
thread and truncated native previews leave the fallback readable.

Native plan deltas and completions use the current live-output owner. Late deltas
cannot extend a completed item. A thread/reverted observation clears display-only
output only when its process generation owns the thread; receipts and execution
facts remain under their existing owners. Process loss retains the active unknown
turn's output; terminal recovery uses the same transaction as normal completion.

## Qualification

Database: 121 unit and 3 integration tests passed. Protocol: 134 unit and 6
integration tests passed. GPUI: 463 passed, 5 opt-in tests skipped. Runtime: 566
unit tests passed, 41 opt-in tests skipped; integration and documentation tests
passed. Final local-history deduplication passed the missing/complete/empty native
readback fixture and 41 application tests (one opt-in test skipped).

Strict Clippy passed for all four packages across all targets and features. The
rendered GPUI test verifies original-source copy and retention until exact complete
native content arrives. Unit tests also reject replacement by another thread,
turn, item or kind, an empty item, a truncated item, or a terminal boundary alone.

The installed Codex 0.155.0-alpha.16.4 plan fixture passed through coordinator/live
storage and paged native history after restart, with one model request. A completed
native plan does not become unfinished fallback. The experimental schema was also
regenerated from this binary. Interruption retention is covered by deterministic
coordinator/database fixtures, not an external-provider interruption session.

The fixed upstream reference is `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Its app-server protocol declares `item/plan/delta` with the native plan delta
notification. Installed-native qualification uses an isolated loopback provider. No production
database mutation, external-provider or installed desktop acceptance is claimed.

