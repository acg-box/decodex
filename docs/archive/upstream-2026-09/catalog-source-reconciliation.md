> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile catalog and effort consumers

The Chief host checked only the retained process generation after model discovery.
The inherited owner also checked the account, account revision and current account
readiness. Restore that source check before and after discovery through the
existing `chief_usage_source` and database account owner. Discard a catalog when
its source changes or becomes unavailable. The host does not start a new process,
write configuration or retry inference to obtain the catalog.

## Whole-file reconciliation

- Runtime `chief_capabilities.rs`: restore the scoped reader and the account-change
  regression. Restore the model-defined effort regression. Keep the current
  long custom-effort/default checks. The feature-discovery regression moved within
  the file but retains its exact task-config and oversized-page assertions.
- Ordinary `conversation/model_catalog.rs`: keep the current shared metadata-process
  owner used by creation discovery and cold model controls. It retains admission,
  selected-directory revalidation and explicit process shutdown. The former raw
  defaults parser now lives in the native adapter's `NativeExecutionDefaults`;
  the runtime only validates the typed public projection. Restore the inherited
  absent/invalid/default matrix through those existing owners and restore exact
  selected-effort-to-turn serialization. Keep current interleaved-event and
  independently rejected configuration/managed reads.
- Desktop `chief_effort_slider.rs`: all inherited slider behavior remains. Current
  changes add native-inheritance and empty-catalog labels, saved explicit intent,
  accessibility selectors and a custom-effort regression. The complete diff was
  reviewed. The already-run full desktop suite includes those retained tests.

The shared host, conversation runtime, protocol modules and desktop surface remain
open for other inherited differences. This batch closes these three file rows
only. It does not count file closure as feature or desktop acceptance.

## Evidence and limits

The restored account-change fixture covers a stable source, changed revision and
lost source. Eleven runtime catalog tests, five ordinary defaults tests and two native
adapter default tests pass. Strict runtime lint passes with all features and
targets. The previous desktop integration run passed 517 tests
with 5 opt-in tests ignored; the slider source is unchanged from that run.

At fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`, the model-list test
uses the configured authentication/provider and explicit catalog enablement. That
source was inspected. No native model discovery flag or endpoint is changed here.
These fixes preserve existing consumers. Optional catalog notices and automatic
fallback policy remain separate review items. No new signed desktop acceptance
or installed-native qualification is claimed. Automations remain paused.
