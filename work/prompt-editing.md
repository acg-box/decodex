# Edit an earlier prompt

Classification: optional product capability. Canonical input selection is
implemented, with a durable journal and service-owned native confirmation/recovery.
The local protocol now carries review, confirmation, recovery and explicit draft
acknowledgement. Desktop presentation, canonical draft storage and editor integration
remain open; this is not a complete editing action.

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
instructions, start model work or mutate history. It returns all chronological turn IDs, the exact latest turn
and the existing connection/history/settings guard. A changed latest turn or
observed revert rejects the read. These are preview evidence, not mutation authority.

The selection uses IDs instead of visible prompt ordinals. The service and UI must
still enforce current visible selection and restorable content before offering a
write. The coordinator enforces internal voice-handoff exclusion and exact
account/process ownership.
An unsupported or non-editable candidate returns None; malformed, incomplete or
changed native evidence returns an error. Neither is permission to use old UI text.

## Remaining delivery

- Present the complete selected input and the history boundary for review. Preserve
  attachments and canonical mentions when restoring an editable draft.
- Connect desktop selection and its existing draft owner to the public commands.
  Save the complete canonical input durably and refresh displayed history before
  acknowledging handback. Do not rebuild a service guard from client JSON.
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

## Durable edit reservation

The existing chief_inbox_events journal owns prompt_edit_attempt,
prompt_edit_observation and prompt_edit_release. Schema43 marks the minimum
reader contract so an older service cannot ignore a pending edit after downgrade.
The versioned migration changes no tables or existing product data. A reservation stores the reviewed native content, complete ordered
turn IDs, selected item/boundary, work/thread identity and process generation.
It accepts only an idle owned task with no queued input or open voice call.
A review token can reserve only once, even with a new request ID after release.

Until release, normal input and dispatch, capacity retry, voice admission, tool
upgrade and model/permission/plugin selection reject the task. An account change
also rejects a root with an unresolved edit in its subtree. Other tasks can
continue. Journal entries stay out of transcript pages and never wake a model.

No reply leaves the reservation unresolved across restart. A positive pre-write
rejection can release the exact original attempt. A timeout or generic remote
error is not that evidence. A guarded complete native read can mark application
only when all turn IDs equal the exact retained prefix. An unchanged full history
can release the attempt only after the old process has confirmed death and a
current process on the same account supplies the observation. Other histories
remain unresolved; neither a missing suffix nor a thread/reverted notification
alone supplies sufficient evidence.

An applied observation keeps input blocked until the service reconciles its
projections and hands back the canonical draft. Runtime must perform those steps
before it calls release_chief_prompt_edit_draft. The store cannot validate native
transport guards or prove UI draft receipt. The native bridge now admits
thread/revert for the coordinator confirmation path. Desktop integration remains required.
Tests prove durable store behavior, ownership and dispatch exclusion, not the
remaining native mutation or signed desktop editing flow.

## Native coordinator confirmation and recovery

prepare_prompt_edit returns an opaque service-held review with canonical content,
all native turn IDs and its live guard. It rejects internal voice handoff input.
confirm_prompt_edit re-reads the exact item and complete history, compares them
with the review, reserves the journal, then submits one guarded native request.
The transport checks the guard immediately before writing. The native API has no
expected-latest compare-and-swap parameter; this is not an atomic cross-client
history lock.

Only positive pre-write errors or native validation/unsupported-method errors
(-32602 through -32600, as specified by the fixed upstream TUI) release the
reservation without history observation. Success, lost reply and other errors
all require a fresh guarded complete read. No recovery path resubmits the write.
An applied receipt proves native history only. Recovery invokes existing request,
output, capacity-retry and question invalidation/rebuild, but keeps the reservation
until the remaining presentation state and desktop draft handback are complete.
Repeated recovery rechecks applied history and retries incomplete projection reads.

Transport tests cover changed canonical input, successful confirmation, validation
rejection, post-commit internal error and lost reply with a new coordinator. They
assert one mutation and read-only recovery. The isolated installed-native fixture
can opt in with DECODEX_TEST_PROMPT_REVERT=1 after the recap checks. It removes only
its final synthetic parent turn, verifies the exact retained prefix and native
settings, and requires an unchanged model request count and a retained input fence.

Installed-native qualification observed serviceTier change from null to "default"
in resume metadata after restoration. Fixed upstream
ModelInfo::service_tier_for_request omits both values from model requests; the
fixture compares that request meaning while comparing other selected settings
exactly. This does not claim raw service-tier metadata is byte-identical or that
a different explicit tier may be discarded.

## Public local protocol

Local protocol2.86 adds PreparePromptEdit, ConfirmPromptEdit, RecoverPromptEdit and
AcknowledgePromptEditDraft. The existing Chief actor owns all four. It keeps at
most eight service-held reviews for ten minutes; stale or consumed reviews cannot
be reconstructed from client input. Confirming the same durable review again only
reports its receipt, even if the caller uses a different command key. These actions
neither rotate an exhausted account nor run the post-command wake path.

GetChiefPromptEdit reads an exact work/thread and returns a 64KiB UTF-8 fragment
of the canonical input JSON. Continuations require the same review token and an
exact byte offset. ChiefClient::prompt_edit assembles at most8MiB within sixty
seconds and rejects changed phases, identities, sizes or offsets. It parses only
the complete array. This keeps large text and image/file evidence below the256KiB
transport frame limit without truncating canonical input.

Applied reports native history evidence, not a restored desktop editor. The client
must persist the returned draft and refresh its presentation before it explicitly
acknowledges the exact receipt ID and review token. The service rechecks native
history and question recovery before releasing the input fence. A duplicate exact
acknowledgement is harmless; another receipt cannot release this edit. Querying,
recovering or acknowledging does not submit the restored draft. The current GPUI
composer still needs canonical binding/file-ID storage and editing support.

## Canonical draft staging and send integration

The current feature branch adds protocol2.87 staging commands and a separate
SendPromptInput command. The desktop editor retains complete parts and text
markers in the existing version8 draft document. Input staging uses immutable
SQLite records and durable 64KiB chunks. A lost staging reply permits progress
only after a read confirms saved bytes. Staging never authorizes a model turn.

SendPromptInput identifies the exact work, thread, edit receipt, immutable record
and digest, with execution settings captured at send time. Queue admission checks
the source and acknowledged draft handback in the same transaction as the existing
user_message event. The event contains a labeled, bounded preview and an input
reference. It does not contain full image data. The existing dispatch owner loads
complete parts, checks their source, applies the captured settings, and uses the
normal dispatch fence and native request path. The preview is not model input.
The input retains its native thread instead of triggering a tool-upgrade fork.

Focused tests cover exact part and marker retention, large image data, execution
settings, bounded queue payloads, repeated admission, pending handback rejection,
and changed thread or digest rejection. These tests construct native request
parameters; they do not prove a complete desktop send or installed-native result.

Desktop confirmation, durable handback, history refresh, explicit send, ambiguous
send recovery, and full native request size qualification remain required before
this optional feature is accepted. Local protocol and database tests do not close
those acceptance requirements.
