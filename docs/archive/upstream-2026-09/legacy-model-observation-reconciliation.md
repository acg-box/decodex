> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile preserved model publication records

The complete inherited `database/src/chief_model_recovery_observation.rs` is
implemented through the existing shared settings transaction, current model owner
and `chief_model_legacy` compatibility reader. Keep one publication entrypoint and
retain the legacy event identities.

| Original obligation | Current implementation and evidence |
| --- | --- |
| Require a known generation and native publication | `record_settings_observation` calls `Kind::observe` only for the publication path. It verifies exact thread ownership. `chief_models::observe` verifies the active owner before calling the legacy reader. Response-only facts remain separate. |
| Require a known tier, including explicit null | `model_facts` rejects missing or malformed tier metadata. `legacy_automatic_unset_tier_requires_an_explicit_null_publication` checks missing, boolean and wrong-tier values before accepting explicit null. |
| Confirm only a later matching publication from the original generation | The legacy reader checks the reservation event precedes the observation, original generation, model and effort. The account revision must still match and the account must be enabled and present. A replacement process records reconciliation only after confirmed old-process death. |
| Preserve the reported tier for manual edits and require the exact expected tier for automatic edits | The six-case legacy receipt fixture checks manual/automatic origin across no response, queued and uncertain responses. Manual edits accept reported null while an automatic priority request remains pending. |
| Do not confirm a rejected request or create duplicate observations | The legacy unresolved predicate excludes rejected, observed and reconciled records. The tests send duplicate matching publications and verify exactly one observation; rejected or changed-account records acquire none. |
| Preserve the original response and record observation separately | Historical reads retain the initial event ID, response and manual/automatic origin. The test checks original payload bytes after database reopen and confirms that observed state never rewrites the response. |

The compatibility path also handles the existing restart-reconciliation contract.
The test proves that a missing old-process death receipt, a stale generation,
incomplete new facts or response-only settings do not release the request. A
valid new-owner publication writes a reconciliation record without an old-target
observation, retains the unknown response after reopen and permits subsequent
work. It does not resend the model update or user input.

This closes only the complete observation-helper file. The old writer, complete
inherited test module and shared service files require their own full mapping.
The latest database model regression run passes 22 tests, and strict database
Clippy passes with all features and targets. See
`/tmp/decodex-legacy-fallback-identity-after.log` and
`/tmp/decodex-legacy-fallback-identity-clippy.log`.

Legacy receipt reading and no-replay guards are core compatibility for saved user
state. Removing the optional selector or automatic fallback policy must not drop
these readers or allow unresolved old operations to replay. Signed desktop and
live native acceptance remain separate.
