# Restore catalog notices and reconcile request presentation

Read the complete inherited/current chief_capabilities.rs and chief_requests.rs
diffs. Keep the current native catalog, explicit input choices and approval owners.
No protocol, schema or native execution policy changes are required.

## Reproduced catalog loss

The inherited missing-model test fails after compilation: an available empty
catalog no longer produces the missing-model notice. The saved explicit model
remains present, but the user receives no explanation. Failure evidence is in
`/tmp/decodex-catalog-missing-before.log`.

Restore the notice only when the current source-bound catalog is available and
the selected model is absent. Unknown or unavailable catalogs do not establish
absence. Changing the account invalidates the notice without replacing the saved
model. Restore the complete inherited rendered regression.

The speed picker had also lost its distinction between inherited task speed and
an explicit next-message tier. Restore selection from the existing execution
choice owner. An absent choice shows the inherited label and marks no explicit
tier as selected. Keep the current configured Flex label, supported catalog tiers
and saving of explicit changes. Extend the existing rendered tier-selection test
to check the inherited label and restored saved-choice invalidation assertion.

## Remaining catalog mapping

Current source-bound discovery retains account/directory/runtime checks and an
independent request generation. It also retains typed creation defaults, clears
them on invalidation, and saves the resulting draft. Explicit model selection
reconciles effort through reconcile_selected_model_effort; a passive catalog
refresh preserves the user's effort and reports unsupported choices. Empty effort
catalogs preserve configured values. These are deliberate current behaviors,
covered by chief_effort_catalog_tests.rs, not lost effort validation.

Catalog access-program labels now display reported identifiers and distinguish
empty from absent metadata. They do not grant access. Upgrade and retirement
notices retain their existing fields. The optional control does not activate a
native feature or rewrite an unlisted model.

## Complete request mapping

The current request reader uses event, generation and revision checks, bounded
UTF-8 sections and previous/next navigation. Explicit decisions no longer require
visiting every page. This product difference is already recorded in
large-approval-reconciliation.md; do not restore a mandatory page-visitation gate.
Complete request content remains available independently of the 4096-byte summary
threshold. Request choice callbacks reject old request revisions.

Question option interaction, request-local timeout snoozing, reloading without
rearming, encoded-answer limits and dispatch failure preservation remain intact.
The large permission test still checks compact decision dispatch without copying
the complete reply into the client command. Reader navigation checks mouse,
keyboard and stale section controls.

Restore the complete inherited native-executor display test. It checks command
and permission approvals, Unicode Windows paths, UNC paths and absent/empty/native
environment identifiers. The narrower current fixture checked permissions only.
This restores coverage without changing approval policy or inferring an executor
from its working directory.

Close only these two shared-file comparisons after focused rendered tests and
strict GPUI validation. Other shared source files, native capability limitations
and fresh signed desktop acceptance remain open. Automations remain paused.

Validation passes 13 catalog/model tests and seven request tests with no skips.
Strict GPUI Clippy passes all features and targets in 6.45 seconds. Logs are
/tmp/decodex-catalog-recovery-final.log, /tmp/decodex-request-recovery-final.log
and /tmp/decodex-catalog-request-clippy.log. These are rendered fixture and static
checks, not acceptance of the final signed desktop artifact.
