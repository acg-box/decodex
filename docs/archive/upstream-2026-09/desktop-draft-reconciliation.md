> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile inherited desktop draft storage

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Read the complete inherited note and complete diffs for desktop_drafts.rs,
desktop_draft_recovery.rs and chief_draft_storage.rs. All inherited function and
test names remain. This batch changes documentation only.

## Retained and extended owners

| Inherited contract | Current owner and difference |
| --- | --- |
| Private revisioned store outside product storage/cache | ClientDraftStore remains the sole file owner. Desktop Storage::open decodes the bounded document; publish_document reloads after WriteUnconfirmed and accepts only matching bytes. |
| Profile, work, thread and question identity | The document retains exact profile namespaces, work/thread editors, explicit choices and unique question identities. The unbound composer has no execution owner. |
| Original input before dispatch | command_draft_copy and fence_command_draft retain a complete original and command identity before publication. Save failure prevents dispatch; busy/conflicting writers retain both disk and editor input. |
| Definite versus unknown outcome | remove_command_draft_fence removes only the exact copy. retain_failed_command_draft preserves changed text or execution choices; uncertainty retains its exact command key. |
| Positive steer receipts | resolve_steer_draft_copies matches service/work/thread/submission, preserves other unknown IDs and leaves saved text available. Text similarity and missing pending state are not receipt evidence. |
| Empty edits, offline input and later edits | The original background publication loop, saved/document comparison, captured revision and recheck remain. Returning results cannot erase a newer editor. |
| Explicit keep-both and restore | The recovery owner starts from latest disk state, preserves unrelated remote profiles, retains displaced inputs and carries unknown delivery into the selected draft. Capacity failure refuses the operation without deleting alternatives. |
| Explicit removal/export | Exact recovered copies with unresolved delivery cannot be removed. Existing recovery/export owners remain separate from submitted-input authority. |
| Quit preflight | flush_drafts_for_quit cancels queued and prepared prompt sends, repeatedly publishes latest input and refuses exit on error or deadline. This is still distinct from physical menu/Dock/application acceptance. |

Four storage tests now use RefreshIntegrations instead of Interrupt to exercise
the ordinary saved-command queue. Interrupt has a separate exact-turn control
path in ChiefSurface::execute and does not create a new user input. The tests
still verify save-before-dispatch, existing-save ordering, profile changes and
conflict refusal. The accepted-command case now includes a real fenced copy and
checks that acceptance removes it. The cold effort case uses an arbitrary native
provider spelling instead of only High. No inherited test name is removed.

## Additions since the inherited document

The current local draft schema is 10, independent of the service wire revision.
It accepts schemas 1 through 10 and upgrades decoded documents to 10. Legacy
ordinary bound editors from versions through 5 keep explicit creation intent;
unbound input does not gain service authority. Creation setup retains incomplete
model/directory/account edits and explicit default/effort/tier intent.

Ordinary editors use exact directories, share the original writer and carry
separate unknown commands. First-profile adoption preserves displaced saved
input. Unbound ordinary records reject conversation IDs, parked conversations
and unconfirmed commands.

Prompt editors are keyed by exact review identity and retain canonical media
outside the occupied main composer. Staging, renewal, handback and settlement
check the saved profile and exact draft. Confirmed send keeps a manually
restorable complete copy; known failure clears only that send's pending state.
Reconciliation preserves remote pending sends and handback constraints while
retaining local alternatives. Capture does not overwrite ordinary or prompt
editors when saving Chief input.

The inherited 4 MiB aggregate note is historical. The current shared limit is
4 times MAX_NATIVE_MESSAGE_BYTES, or 32 MiB. It accommodates canonical input and
retained copies. The existing 32 recovery-copy bound, 64 profiles, 16 KiB ordinary
text bound and bounded encoder remain. The aggregate test now fills two profiles
with 256 parked editors to exceed the actual current bound. This is an explicit
capacity change, not byte-equivalence with the inherited policy.

## Evidence and remaining acceptance

The three reviewed source files are byte-identical to the versions validated with
PR1630's complete suites. That run passed all 12 desktop_drafts protocol cases
and all 39 registered desktop drafts::storage cases, including child modules.
The broader run passed 165 protocol and 532 GPUI tests, with five opt-in GUI tests
ignored. No additional test rerun is needed for this documentation-only mapping.

Read the current save/publish/quit paths, complete recovery rules and ordinary
unbound validation. Verify all four inherited snapshot hashes. Close these three
complete source rows and the inherited document row. This does not close the
shared conversation/shell owners or R07/R12. The signed 0658 run proves only its
recorded normal quit/relaunch and draft restoration subset; Dock, keyboard,
conflict-cancel, export, broader blank-task/worktree behavior and app-owned service
shutdown still need their stated acceptance. A fresh signed artifact is required
for later production changes. Automations remain paused.
