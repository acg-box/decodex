# Reconcile the inherited database root

The complete `database/src/lib.rs` diff is mapped below. This file registers
existing persistence owners; it is not a second database implementation.

| Difference from the preserved snapshot | Current owner |
| --- | --- |
| Missing `AccountNudgeReceipt` export | Restore the original public type path. The existing `account_lifecycle` receipt reader and command journal remain unchanged. |
| Separate App exposure attempt/receipt module removed | `chief_app_settings` and `chief_config_journal` own shared App writes. The read-only legacy exposure outcome remains in `chief_app_settings`; see the inherited App exposure disposition and legacy outcome tests. Old writers are not reintroduced. |
| Model recovery, status and observation modules replaced | `chief_models` registers `chief_model_history` and `chief_model_legacy`. The current attempt includes explicit manual-source and automatic recovery context; retained legacy records and pending-write rules use this owner. See [model owner](model-owner-reconciliation.md) and [history](model-selection-history.md). |
| Task-settings module replaced | `chief_settings_observations` exports the same observation type under the current shared settings owner. See [settings reconciliation](shared-settings-observer-reconciliation.md). |
| Additional module registrations and exports | Existing App UI calls, shared config, hooks, prompt edits/inputs/uploads and voice history expose their current owner DTOs. Existing install, Guardian, native turns, permissions, plugins and live settings remain registered. |
| Output revision sender in both store constructors | Both constructors initialize the same per-store watch sender. `chief_output` and `chief_reasoning_summary` notify after successful changes; `wait_chief_output` subscribes before reading. Durable output remains in SQLite. |
| Desktop default fixture | The expected settings value adds `auto_recap: false`. The existing default and migration assertions remain. |
| Account route diagnostic regression | The existing receipt/restart case now records the credential-negative stage and cause and verifies replay after reopening. See [route diagnostics](account-route-failure-diagnostics.md). |

All other inherited root-file code remains unchanged. This closes the root-file
mapping only. Broader migration, account, conversation and Chief persistence files
retain their own rows. Historical writer APIs are mapped to current canonical
owners; the root mapping does not claim source compatibility for retired writers.
Native execution, notifications and signed desktop acceptance require their own
evidence. Tests use disposable databases and no real account notification.

Validation passes all 174 database unit tests, including the receipt/restart and
output/migration cases, and strict database/runtime Clippy for all features and
targets. Logs: `/tmp/decodex-account-receipt-api-database.log` and
`/tmp/decodex-account-receipt-api-clippy.log`. The original root-file hash matches
the preserved snapshot. No new schema version or migration is added.
