# Reconcile optional integration controls

Compare the complete inherited connector-exposure, task-plugin and model-access
notes with current source. These are three implemented optional product controls,
not three requirements to build a second native runtime.

| Control | Current behavior | Removal boundary and qualification limits |
| --- | --- | --- |
| Connector tool exposure | `chief_app_exposure` and the native App adapter preserve inherited versus explicit-empty omissions, source/version review, explicit Save and shared App/Hook receipts. Legacy outcomes remain readable without replay. | Remove the editor and its dedicated query/command consumer together if unwanted. Preserve the shared journal for retained App/Hook writers. A config acknowledgement does not prove a live model tool list changed. |
| Task plugin selection | `chief_plugins` and the current selection service preserve other exclusions, exact plugin IDs, source review and durable queued/unknown outcomes. The UI reads state after a write or lost response. Native activation controls later turns. | Remove the per-task selector without replacing native plugin filtering. Shared installation, canonical connector selection, enterprise registration and Sites loading are separate native responsibilities. |
| Model access metadata | The catalog and model detail display preserve known advertised programs, absent versus empty values and changed observations. They do not select a program, change quota or grant access. | Remove the informational display and dedicated metadata projection as one optional unit. Keep model catalog ownership, permissions and native routing. There is no Decodex Daybreak preference control. |

## Complete inherited record mapping

Connector exposure now uses the shared App/Hook journal instead of the old
exposure-only writer. The [App settings reconciliation](app-settings-owner-reconciliation.md)
preserves historical outcomes and distinguishes a save acknowledgement from
subsequent readback. The current explicit integration refresh retains the exact
thread through plugin reconcile, MCP reload, complete App discovery and forced
`app/installed`. Partial or failed stages do not acknowledge the whole operation;
ordinary inventory reads do not force refresh. Source:
`app_server_client/integrations.rs::refresh_integrations`.

Restore the original task-plugin record unchanged below a current status notice.
The note's adapter, store, service and UI claims map to current owners documented
in [native plugin qualification](native-plugin-controller-qualification.md) and
[settings observation mapping](native-settings-observation-tests.md). The current
transport revision model supersedes raw-value duplicate identity. Historical
fork, cross-client and native catalog probes retain their stated limitations.
Enterprise MCP registration, canonical shared connectors, root-only installation
suggestions and Sites cache precedence remain native-owned; this batch does not
claim new live qualification of those paths or recreate their policies locally.

The model-access note retains its catalog/projection/UI contract. Restore the
distinction between initial native Daybreak preference and displayed access
metadata. Its historical protocol and binary versions remain historical.

## Evidence scope

Fresh targeted validation covers current plugin-service receipts, exposure
source/receipt behavior and access-metadata projection. A separate installed
native catalog test covers access metadata refresh across cold starts with a
synthetic account and loopback provider. These are not real enterprise grants,
external connector acceptance or signed desktop interaction.

Results: one plugin-service test, two exposure tests, one access projection test
and one installed-native access test pass with no skips. Logs are
`/tmp/decodex-optional-plugin-owner.log`, `/tmp/decodex-optional-exposure-owner.log`,
`/tmp/decodex-optional-access-owner.log` and `/tmp/decodex-optional-access-native.log`.
The native test uses Codex `0.158.0-alpha.2.1`, SHA-256
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.

Close only the three document rows after snapshot verification and complete
mapping. Keep the remaining shared GPUI source review and signed desktop
acceptance open. This inventory supplies removal choices; it does not remove
features or expand optional scope. Automations remain paused.
