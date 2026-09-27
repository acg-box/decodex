# Reconcile the shared Chief workspace

Compare the complete inherited/current chief_workspace.rs diff at main
`030409f62d2af9fbc3a80b2c3411ad771500c53d`. The preserved source hash matches the
register. Keep the fixed upstream cutoff
`595cc91e8cbb1c2ca822d0311dcf12709410c582`.

## Complete mapping

| Difference | Current owner and disposition |
| --- | --- |
| Navigation | Opening a page leaves the native-child view. Changing work clears model, reviewer, permission, plugin, hook, app, goal, activity, recap and prompt-edit observations before selecting the next owner. Existing draft ownership and history cache remain. |
| Buttons and labels | Prompt buttons permit wrapped content. Project creation moves into the Projects heading and still preserves an occupied draft. Main/Agent wording replaces Overview/Worker. Sidebar tooltips are removed; accessible labels remain. Restore the inherited held-key guard for Enter and Space. |
| Unavailable conversation | Connection failure is checked even before a task is selected. The locked-thread notice is compact; the recovery editor remains below it. The existing submission guard preserves drafts and prevents dispatch. |
| Transcript | workspace_transcript extracts the existing scroll/history/welcome renderer. selected_workspace_item becomes an inline exact selection lookup. History rendering and pending-question detail remain. |
| Native goal | The former transcript goal display moves to the existing preferences renderer and explicit native-goal read. It remains a read-only optional observation, not a local execution goal or an automatic inference request. |
| Work status and details | work_context replaces worker_status with the current work label and graph-derived state. The inspection card becomes an overlay. Graph mouse input records the focused panel for existing resize commands; conversation input clears that focus. |
| Native child view | Existing child polling and visible-output observation run through their retained owners. A selected native child uses native_agent_view and hides the manager composer. No local worker is created by this rendering path. |
| Draft recovery | Recovery copies precede worker activity. has_unconfirmed_delivery includes prompt-edit handback/send and ordinary-input receipts as well as the original uncertainty flag. Ordinary directory previews and creation settings appear in the recovery summary. |
| Activity | Remove duplicate sending/running labels from the activity footer; the primary control and work header carry those states. Keep compaction, failure, disconnected and uncertain-delivery notices. Restore an explicit stop control on a running worker's footer because that page has no manager composer. |
| History position | Prefetch older history through the existing paused-follow owner. Restore a message-position anchor, with maximum-offset fallback and bounds clamping; clear it after convergence or a task switch. Latest-follow uses the existing frame scheduler. These changes do not replay messages. |
| Visual fixtures and tests | Retain all original test functions. Update fixtures for native history fields and add prompt/recap examples. Added recovery tests cover editable disconnected drafts, empty workspaces and uncertain delivery. |

The shared model, native-child, history and prompt owners retain their separate
contracts. Optional settings, recaps and prompt editing remain subject to the
user's subtraction review. This comparison does not authorize new product scope.

## Repairs and validation

The held-key rendered regression fails before repair: one click followed by a
held Enter invokes the action twice. Restore the original !event.is_held guard;
held Enter and Space now do nothing, while fresh presses still activate it. The
initial attempt used an inspection overlay whose click did not establish the
required state; it was replaced by a view of the actual workspace_action control.
Only the latter failure establishes the held-key defect.

The running-worker regression fails before repair because its rendered footer
has no stop control. Restore that control through interrupt_current and the
existing independent cancellation owner. Show it only for the selected running
work with an exact turn and a ready connection. The test confirms presence,
selected-worker routing, draft preservation and absence after disconnection.
It does not claim a live provider interruption.

The retained runtime interrupt_work checks the stored active turn before issuing
native turn/interrupt with threadId and turnId. The fixed upstream common.rs
request mapping and app-server test helper use that same method. The previously
generated installed-binary TurnInterruptParams schema also requires both fields.
No native transport or capability has been added by this UI repair.

All nine workspace tests pass. The complete desktop binary suite passes 534 tests
with five existing ignored cases; strict stable Clippy passes with all features
and targets. The original file hash and git diff --check pass. No live account or
provider is used. Close this complete file disposition only. Shared-source review,
final signed desktop acceptance and the user's optional-feature decisions remain
open. Automations remain paused.
