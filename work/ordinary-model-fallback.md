# Restore ordinary model fallback

This batch restores the inherited automatic fallback policy through the current
model-selection journal. It is an **optional Decodex policy** for the final user
review. Native settings correctness is required if this policy remains.

When a current account banner blocks the configured model and names an available
ordinary alternative, the existing Chief actor can update an idle task's saved
model and effort. It checks one rotating candidate on its existing 15-second
tick, with a 12-second observation limit. Native authentication must report
ChatGPT. The banner, account revision, process generation, task, native settings
and drained event queue must remain current before and after reservation.
Custom providers, unknown support, Reserve, running tasks and queued explicit
execution choices do not qualify.

The update preserves supported effort and service-tier semantics. It uses only
`thread/settings/update`; it does not send or replay conversation input. An empty
RPC reply means queued, not applied. Only a matching native publication can
confirm the target, including the expected tier and account revision. Unknown
delivery stays unresolved. A new request identifier, process or account refresh
cannot replay the same reserved fallback. A new owner can supersede an old
attempt only after confirmed process death; it cannot attribute the old request
to current settings.

## Owners and compatibility

- Restore `app_server_client/model_recovery.rs` byte-for-byte from the preserved
  snapshot. Keep the separate manual model-selection payload unchanged.
- Adapt `chief_model_recovery.rs` to `chief_models` and `ChiefModelAttempt`.
  Reuse the current reservation, observation and pending-edit checks. Do not
  restore a second writable model-recovery journal.
- Add optional recovery provenance to the existing attempt payload. Old manual
  payloads retain their serialization and identity. No schema migration is needed.
- Retain the inherited native model-recovery fixture and all its assertions.
  Adapt only the shared Responses fixture helper.

Legacy pending recovery records still block conflicting work. Historical terminal
receipts and manual/reconciled distinctions are now restored through the current
owner; see [model selection history](model-selection-history.md) and the complete
[writer mapping](model-owner-reconciliation.md). Shared module rows and signed
desktop acceptance remain open.

## Upstream and native evidence

The fixed upstream commit remains
`595cc91e8cbb1c2ca822d0311dcf12709410c582`. Its app-server protocol and settings
tests define partial updates. The installed binary is `codex-cli
0.158.0-alpha.2.1`, SHA-256
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
Its experimental generated schema confirms optional model/effort and the
distinction between an omitted tier and an explicit null tier. The default
schema omits experimental methods and is not a support test.

The installed-native fixture passes four cases: default/plan collaboration mode
with an explicit/preserved tier. It preserves instructions, permissions,
reviewer, sandbox and config bytes. The settings update and cold resume cause no
inference. A second explicit input makes exactly one additional request using
the target settings. Log: `/tmp/decodex-model-fallback-native.log`.

The service fixture covers 12 scenarios: queued, unknown, rejected, preserved
tier, custom auth, unavailable banner, Reserve, source changes before/after
reservation, banner changes before/after reservation, and queued explicit input.
It uses the actual SQLite owner and a synthetic native protocol peer. It checks
exact request payloads, durable receipts and no replay after reopen. The policy
also retains all three inherited effort/tier/event-queue tests. Model-focused
regressions pass 18 adapter tests and 23 runtime tests; 16 opt-in runtime tests
are ignored in that default run and are not counted as passes. Strict Clippy
passes for database, adapter and runtime with all features and targets.

The native fixture qualifies partial native updates with a synthetic provider;
the service fixture qualifies automatic ChatGPT eligibility with synthetic
metadata. Neither proves a live account fallback or signed desktop acceptance.
No user configuration, installed application or automation is changed.

A separate inherited Flex fixture finds a restart limit in the installed binary:
an explicit settings update uses Flex live but resumes with a null tier. Configured
Flex survives. The four passing cases above cover standard/preserved initial tiers,
not that explicit Flex case. See [the open qualification](native-flex-qualification.md).

## Removal boundary

To remove automatic fallback, remove the Chief tick hook, observation attachment,
policy and dedicated automatic transport together. Preserve the current manual
model-selection owner, native publications and readers of saved recovery records.
Retain no-replay handling for reservations that already exist. Account recovery
notices and explicit user choices are separate features.
