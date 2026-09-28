> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile the ordinary conversation controller

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Read the complete 4,422-line comparison between the preserved snapshot and the
current controller, including the restored model-review fixtures. The preserved
file hash matches the register.

## Reproduced loss

`remove_task` removed the archived conversation's live deltas but did not update
`live_delta_bytes`. If those deltas filled the cache, the next conversation's
output was evicted immediately. Restore the inherited two-line byte-count update
and the removed assertions in `archive_result_removes_the_exact_selected_task`.
Before repair, the expected next output was absent. The failing test is recorded
in `/tmp/decodex-conversation-archive-before.log`.

## Source mapping

| Difference | Current owner and disposition |
| --- | --- |
| Model and effort choices | Retain the native catalog when available. The optional offline picker uses the existing curated list. Explicit per-field intent replaces the old model/effort pair flag. Nullable reasoning preserves native inheritance. Restore the provider-defined effort test, including its encoded command round trip, with the current optional type. |
| New-conversation defaults | `creation_defaults::resolve` owns precedence; `conversation_creation_defaults_tests` checks one discovery per context, missing defaults, source invalidation, managed values and explicit choices. The removed no-loop test is covered there. The old pair-only defaults fixture is superseded by per-field intent and nullable reasoning. Selecting reasoning opts out of the managed pair while service-tier intent remains independent. |
| Catalog loss | The former no-op picker policy is replaced by explicit offline selection. It is an optional UI behavior, not proof that a provider supports the selected value. Native catalog tiers remain source-bound; unadvertised new tiers are rejected. Existing configured Flex remains intact. |
| Existing conversation settings | `conversation_model_settings` binds observations and explicit choices to the selected or restored editor owner. A different conversation cannot borrow prior settings. Foreign or stale replies are rejected; restored observations require a fresh read. Explicit overrides travel separately from inherited fields. |
| Durable delivery | `conversation_drafts` retains the original envelope and requires the exact saved envelope before dispatch. Reconnect clears observations, preserves unconfirmed commands and never automatically replays them. Cancellation applies only while the queue still owns the command. |
| Uncertain outcomes | `conversation_turn_recovery` and `conversation_control_recovery` own exact result checks. The renamed send-failure and disconnect tests now require the original message outcome; a current conversation list alone no longer permits another send. Creation, turn and control receipts require their own identities and positive evidence. |
| Routing successors | Restore the original source and follow the exact redirect. Accept newer source/successor revisions without accepting an older one. Reconciliation does not acknowledge unrelated unconfirmed input. Warm and cold fixtures retain the original command without replay. |
| Event publication | Warning-only history now uses `ConversationHistoryChanged`, emitted by the runtime application. Adapt the inherited warning test to that event and preserve the active command and turn. Full conversation projections retain their separate command-state handling. |
| List, archive and refresh | Keep complete-page replacement, exact archive results, bounded refresh batches and older-projection rejection. Missing owners do not retarget unsent input. Repair the archive cache accounting described above. |
| Test movement | Most deleted test blocks moved within the same module. The model-review helpers and rendered draft test were restored separately. Restore the provider-defined effort and warning-event cases here. Keep the current exact-outcome recovery tests instead of reinstating the obsolete list-only retry policy. |

## Validation and scope

All 59 conversation tests pass, with none ignored, including the restored archive,
provider-defined effort and warning-event cases. The full desktop binary suite passes 537 tests with five existing opt-in tests
ignored; strict stable desktop Clippy passes for all features and targets. Logs
are `/tmp/decodex-conversation-reconciliation-tests.log`,
`/tmp/decodex-conversation-reconciliation-all-tests.log` and
`/tmp/decodex-conversation-reconciliation-clippy.log`. This closes this controller's source comparison only.

The default resolver and current per-field behavior do not claim identical
semantics to the old combined intent flag. Offline model controls and the optional
History destination remain choices for the user's removal review. Safe delivery,
source-bound settings and preserving other conversations' output are core
correctness. Installed-native qualification, remaining shared files and signed
desktop acceptance remain separate. Automations stay paused.
