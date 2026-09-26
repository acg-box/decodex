# Edit an earlier prompt

Classification: optional product capability. Canonical input selection is
implemented. Native mutation, durable uncertain-outcome recovery and desktop draft
restoration remain open; this is not a complete editing action.

## Native authority

Fixed upstream commit: 595cc91e8cbb1c2ca822d0311dcf12709410c582. Inventory1371,
ffae979216bfbe94070bd21868d1695277105a63, changed editing from a fork to an in-place
revert. The fixed TUI backtrack selector rejects steers and running turns, excludes
hidden review input and restores canonical mention bindings. The thread processor
and installed Codex0.158.0-alpha.2 schema require a paginated thread for
thread/revert(threadId,beforeTurnId).

That operation removes the selected turn and every later turn from persisted
conversation history. It keeps the thread identity/settings/goal and does not undo
file changes. Returned turns are empty and the retained prefix must be reloaded.
No compatibility alias for the removed thread/rollback operation is needed.

## Read-only selection

AppServerClient::prompt_edit_candidate takes exact thread, turn and item IDs.
It reuses the current native header/item readers and their cursor/frame limits,
with a 60-second overall deadline. Only paginated history qualifies. It reads all
items of the selected turn before deciding whether the selected item is its first
user input. An earlier clipped user item therefore makes a later item a steer.
The selected and latest turns must be terminal. Same-turn review input and the
native reconstructed nested-review pair are excluded.

The candidate retains the complete native content array, including text_elements,
text spans, skill/plugin/app mentions, local image paths and native file IDs. The
reader does not resolve files, download attachments, interpret prompt text as
instructions, start model work or mutate history. It returns the exact latest turn
and the existing connection/history/settings guard. A changed latest turn or
observed revert rejects the read. These are preview evidence, not mutation authority.

The selection uses IDs instead of visible prompt ordinals. The service and UI must
still enforce current visible selection, internal voice-handoff exclusion,
restorable content and exact account/process ownership before offering a write.
An unsupported or non-editable candidate returns None; malformed, incomplete or
changed native evidence returns an error. Neither is permission to use old UI text.

## Remaining delivery

- Present the complete selected input and the history boundary for review. Preserve
  attachments and canonical mentions when restoring an editable draft.
- Revalidate the reviewed item/content and current source before submitting one
  native revert. Add the native mutation to the retained bridge only with its
  service-owned uncertain-outcome boundary in place.
- Persist enough input/boundary evidence before submission to reconcile a lost
  reply or service restart. Never retry a possibly committed revert.
- After confirmed mutation, consume thread/reverted and replace removed history,
  questions, approvals, queued autosend/capacity retry, live output, usage and voice
  replay state before accepting more input. Keep unrelated tasks and durable
  submission receipts intact. Restore a draft without sending it.
- Qualify first-visible/unloaded prefixes, cross-client changes, lost replies,
  restart recovery, retained native settings/goal and signed desktop restoration.

## Validation

Transport fixtures cover complete multi-page input, clipped steers, legacy/running
turns, hidden inline/nested review, canonical attachment/mention fields and new-turn
or revert races. Only native read methods are admitted by those fixtures.
The installed-native fixture also checks the earliest and latest inputs of a
nine-turn paginated thread, exact native content and unchanged request counts.
These checks send no revert to any thread. Existing invalidation tests and this
selection reader do not prove the remaining editing lifecycle.

The maintenance automation remains paused. This optional feature remains part of
the authorized manual pass and the later user subtraction review.

## Core revert observation: capacity retries

Native thread/reverted identifies only the thread. It does not give the removed
turn range. Cancel all still-pending capacity retries for that exact owned thread
in one database transaction. Their previous continuation context is no longer a
safe automatic input. Keep claimed/submitted attempts and delivery receipts.
Do not report a worker completion or wake a manager as a side effect of observing
a revert. This is core native-history correctness and remains useful if the
optional editing UI is removed.

The existing archive restoration path has no durable history-edit journal. The
config journal arbitrates a shared file and is not a history-mutation owner.
Neither is sufficient evidence for admitting thread/revert. A later history-edit
receipt must retain its canonical draft, native boundary and process ownership,
and must block further input until an uncertain outcome is reconciled.
