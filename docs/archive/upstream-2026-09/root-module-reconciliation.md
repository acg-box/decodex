> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile core, protocol and runtime roots

The complete preserved/current diffs of the three crate roots were reviewed.
This maps registrations and exports. It does not close the separate shared
service, wire, client or desktop files, nor prove acceptance of each feature.

## Core root

`MAX_NATIVE_MESSAGE_BYTES` retains the original 8 MiB value. The added
`MAX_APPROVAL_ENVELOPE_BYTES` is two native parts plus 64 KiB metadata; the store
checks each part independently. See [approval reconciliation](large-approval-reconciliation.md).
All other domain modules, exports, foundation ports and tests remain unchanged.

The client-draft module and its four exports moved to the end of the file and
lost their original `cfg(unix)` annotations. The implementation still uses the
Unix path owner. The same change occurs for desktop draft registrations/exports
in the protocol root. On the repository's same-UID Unix client and macOS desktop
scope, the same owners remain compiled. This is not cross-platform equivalence
or a claim that non-Unix builds work. No Windows port or new storage backend is
introduced by this reconciliation. Retain that platform limitation explicitly.

## Protocol root

| Inherited root difference | Current mapping |
| --- | --- |
| Requested decisions, steering and execution move | The same modules and original exports remain registered. |
| Task-model types replaced | `chief_models` exports selection state, original response and historical receipt types. See [model history](model-selection-history.md). The old writer/type spellings are not parallel APIs. |
| Goal result replaced | `chief_native_goal` provides source-bound task/thread readback with an observation timestamp. The service rechecks the complete source key before publication. See the detailed distinction below. |
| App settings and current-turn reviewer types | Existing App reviewer/configuration types remain. The separate `ChiefReviewer` and live settings types belong to current-turn controls. Shared config receipts and saved-connection types are additive. |
| Additional feature modules | Prompt drafts, edits, sends and uploads; task recaps; native agents; App UI resources/calls/receipts; hooks; ordinary creation receipts and turn outcomes; weather; ordinary draft state. Each uses its existing module, not a duplicate implementation. |
| Chief/history exports | Add source identity and output result DTOs; retain the other existing history, request, resource, usage and timeline exports. |
| Local protocol version | Revision 2.94 replaces 2.64. Exact-version admission remains intentional. Do not claim old clients can use the new wire contract. |
| Draft exports | Existing draft owners remain, with creation intent/setup and ordinary draft types added. The Unix annotation difference is recorded above. |

The fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`codex-rs/app-server-protocol/src/protocol/v2/thread.rs`, defines six goal statuses
and signed 64-bit counters. The current adapter matches those definitions and
rejects negative usage and non-positive explicit budgets. The inherited DTO used
an arbitrary bounded status string and unsigned counters; these shapes are not
identical. Unknown future statuses are unavailable until supported. Unbound
sources yield unavailable; disabled and unsupported native APIs stay distinct.
A successful null goal remains distinct from failure.

The current read checks native child ownership and source changes. It redacts
sensitive objective text or bounds it to 8 KiB with `objective_truncated`, instead
of the older 64 KiB presentation bound. It creates no local goal persistence and
sends no goal mutation. These are explicit presentation/contract differences,
not evidence that all old validation rules are identical.

## Runtime root

The old `chief_task_model_selection` registration moves to `chief_models`, as
mapped in [model ownership](model-owner-reconciliation.md). Added private modules
register existing App UI calls/receipts, shared configuration, hooks, native
goals, recaps and native agents. `PromptEditReview` is re-exported from the
existing Chief prompt-edit owner. Other modules, public exports, composition
ports and tests remain unchanged. Account/process internals remain crate-private.

## Verification and limits

Current-source tests cover core and local protocol contracts, including draft
encoding and exact version admission. The native-goal service fixture checks
root/child ownership, missing/disabled/unsupported results, changed source fields,
closed sources, objective redaction and long content. Its local transport uses
synthetic replies and must not be called an installed-native acceptance run.
No runtime or protocol behavior is changed by this documentation batch.
Full feature, non-Unix, native binary and signed desktop acceptance remain separate.

Fresh validation: 88 core and 164 protocol tests pass, plus the 17-case native-goal
service test. Logs: `/tmp/decodex-root-module-contracts.log` and
`/tmp/decodex-root-native-goal.log`. The original snapshot hashes of all three
root files match. These root-file reconciliation rows close. The separate conversation DTO mapping
below closes one additional row; broader wire/client/service rows remain open.

## Ordinary conversation DTO

The complete `crates/decodex-protocol/src/conversation.rs` diff was also read.
Existing model, reasoning, path and thread-identity validation remains. The
provider-thread tests move unchanged; comment wording and match-arm order do not
change those contracts. The substantive differences are:

- Reasoning effort becomes optional so absent/null inherits native state. The
  existing explicit constructor wraps its argument in `Some`. Literal `none`
  and provider-defined effort strings remain explicit values. The dedicated
  inherited-execution test covers these distinctions.
- `ConversationResult::Archived` reports current local archive state and revision.
  It is explicitly not a receipt for a particular client command. The existing
  routing-successor result remains separate.
- Model settings readback distinguishes native provider/model/effort observations
  from the same live session's requested tier. A cold read cannot invent that
  requested tier. `Unavailable` remains distinct from inherited/unset values.
- Explicit execution-override flags remain separate from saved legacy execution.
  The wire regression preserves original legacy bytes and separately round-trips
  modern intent, with distinct command identities.
- The canonical UUID helper changes from parent visibility to crate visibility
  for current protocol consumers. The validation body remains unchanged.

All of this DTO source was covered by the same fresh 164-test protocol run.
This mapping does not qualify ordinary desktop navigation, live native recovery
or archive command acknowledgement. Those owners keep separate acceptance work.
