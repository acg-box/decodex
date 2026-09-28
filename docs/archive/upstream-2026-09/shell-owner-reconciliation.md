> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile desktop shell owners

Read the complete 2,156-line inherited shell diff. The source was unchanged from
the comparison base before the model-review test restoration. The preserved
snapshot hash matches. This note accounts for the shared shell file; signed
application acceptance remains separate.

## Restored account entry

The compact account row hid reauthentication unless `account_needs_login` was
true. The inherited row allowed an explicit login refresh for a healthy account.
Restore that entry while retaining the compact row, existing login availability,
exact account ID, revision and recovery-operation checks. Healthy accounts use
`Refresh account login`; accounts that need recovery use `Sign in again`.

A rendered test fails before repair because the healthy account has no login
control. It passes after repair for both healthy and failed-auth states. The test
only renders synthetic account data; it does not log in or change an account.
Reuse the existing visual account fixture in test builds and add a debug selector
for the existing control.

## Complete source mapping

| Difference | Current owner and disposition |
| --- | --- |
| Navigation and keyboard | Chief is labelled Main. Escape dismisses the open status panel before delegating to the focused Chief interruption owner; held keys are ignored. Panel resize commands delegate to the existing panel owner and account for macOS shifted punctuation. Back/forward and settings-window behavior retain their existing tests. |
| Ordinary drafts | Synchronize through `ordinary_drafts` before quit checks, flushes, selection updates and controller refresh. Profile binding precedes context seeding. Missing task owners remain represented by the draft owner. Creation and turn receipt controls use the current durable controller. See [conversation reconciliation](conversation-controller-reconciliation.md). |
| Account layout | Remove the separate Manage popover and place its actions inline. Profile content moves to the account-summary click; profile and Reset Cards render under their selected row. The power control retains explicit enabled state and keyboard activation. Route, reorder and logout continue to use existing handlers. Logout confirmation remains explicit. Restore the healthy-account login entry as described above. |
| Account identity and quota | `account_identity` owns optional email display and visibility reset on profile change. Readiness labels replace the old colored status dot and short-ID display. Token values use the existing compact formatter. Quota controls render in Accounts through the retained settings owner. These are presentation changes, not new account authorities. |
| Settings | General and Appearance share the existing settings surface; Accounts and Diagnostics retain their own panels. The removed `ChiefPreferences` wrapper was a renderer adapter. Its `render_preferences` content is still reachable from the composer's `agent-settings` menu in `chief_composer.rs`. No second preferences store is introduced. |
| Model controls | Show nullable inherited effort and explicit offline controls through the conversation controller. Defaults and native settings readiness have separate notices; receipt and saved-draft recovery controls keep their current owners. Catalog-defined tiers, Fast and configured Flex retain their distinct rules. |
| Connection updates | A generation change invalidates Chief state. An online cursor advance refreshes it without pretending the connection changed. Recovery polling occurs before dependent view updates. |
| Recap | Automatic polling requires the setting, Chief destination and an online connection. This source predicate does not prove physical foreground-window, live-voice or signed lifecycle acceptance; those remain in R06/R07. |
| Tests | Most inherited test blocks moved without removing their cases. The missing rendered model-review case and its fixtures are restored in [draft coverage](model-review-draft-coverage.md). New tests cover receipts, native defaults, panel sizing and keyboard routes. The removed account helper names map to the inline controls and readiness presentation above. |

## Validation and classification

The failing-before log is `/tmp/decodex-shell-login-before.log`. All 363 Shell
tests pass with one existing opt-in test ignored. Strict stable desktop Clippy passes for all features and targets. Logs are
`/tmp/decodex-shell-account-after.log` and
`/tmp/decodex-shell-account-clippy.log`. No signed desktop or real account login is claimed.

Account recovery access, draft preservation and connection identity are core
correctness for existing consumers. Compact account presentation, email display,
panel sizing, History, recap and advanced settings remain optional product
surfaces for the user's removal review. Close only the shell source row;
other shared-file and final acceptance requirements remain open.
Automations remain paused.
