> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Model review regression reconciliation

The inherited model-settings test file contains five scenarios. The first two
observation tests already exist byte-for-byte in `chief_model_settings_tests.rs`:
source changes, null/missing metadata, malformed provider data, exact settings
invalidation and rejection of a foreign thread. Both pass in the current tree.
Do not duplicate them in the restored file.

Restore the two complete live-model tests with their original assertions. The
publication test covers applied and lost replies, a disabled native feature,
unsupported effort and a changed source. It sends only model and effort to the
exact running turn, records applied or unknown after SQLite reopen, rejects
ineligible edits before reservation and sends no automatic retry or config write.
The choices test binds the task feature and model catalog to the exact source and
discards the choices after a source change. Both restored tests pass.

The fifth inherited test uses the retired model-recovery journal for manual task
selection. The current `chief_models` service and its separate journal cover
source changes, stale reviews, exact model-only requests, lost replies and
restart. Extend that existing test with a successful acknowledgment that has no
settings publication. Its receipt must stay queued and its review cannot replay.
Use a non-null priority tier in the fixture; the selection request still omits
service tier. All five current service outcomes pass. Strict runtime Clippy also
passes with all features and targets.

This maps the current command behavior; it does not prove migration or continued
interpretation of old persisted model-recovery rows. Keep the original file's
complete disposition open until that legacy-journal review is resolved. Its first
four scenarios are accounted for, and the missing current queued/unobserved case
is now restored. Shared reviewer ownership, legacy recovery and signed desktop
acceptance remain open. No production policy, feature flag, schema or automation
changes occur. The explicit model controls remain optional for user review.
