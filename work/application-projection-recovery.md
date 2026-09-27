# Restore inherited public-output filtering

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
The complete 1,954-line application.rs comparison was read against main a058dd53c;
the source is unchanged from the earlier comparison base c02b45a2e. The preserved
file hash matches. This batch repairs two observed losses without closing the
remaining shared-file review.

## Reproduced defects

The public live-output projection lost the inherited reasoning-summary credential
filter and unknown-kind exclusion. A regression feeds a synthetic credential
marker, an unsupported output kind, a plan and an assistant answer. Before repair,
the synthetic marker reaches the public response. Restore query_chief_live exactly
from the preserved file. Sensitive summary text becomes the existing omission
notice and is marked truncated; unsupported kinds are skipped. Public plan and
answer text, identities and byte budgets remain intact. This restores the existing
detector's behavior; it does not claim universal credential detection or alter
stored history.

The saved-history renderer also lost its reasoning_voice_handoff exclusion.
Those records bind native voice provenance and are not conversation messages.
Restore the removed native-warning regression and include a provenance event.
Before repair, it returns two displayed rows instead of one. Restore the original
match arm so the native warning remains visible and the internal marker stays out
of the public timeline. The underlying provenance record remains stored.

## Remaining application review

The full diff also includes reordered query helpers, native goal/model query
renames, ordinary receipt/model-source handling, archived versus missing states,
history-change events, optional recap settings, App UI and prompt input routes,
bounded request details, partial-output identities and question provenance.
Reading those hunks does not by itself close their behavior or test coverage.

The subsequent [approval recovery](application-approval-recovery.md) restores
command-executor coverage, background/child approvals, enriched-diff pagination,
model-review projection and connector/link metadata. The removed authentication
history helper has no call site in the preserved Rust snapshot; it only filtered
the retained test renderer by `auth_recovery`. No production route was removed
with that helper. The oversized detail policy and remaining query-owner mapping
still need an explicit disposition before closing this shared file.

## Validation

Both reproduced failures use local synthetic data and no live credential or
provider. The application suite passes 50 tests with four existing opt-in tests
ignored. Strict stable runtime Clippy passes for all features and targets;
git diff --check passes. Keep the application file's overall disposition
open. Final signed desktop acceptance and the user's optional-feature inventory
remain separate. Automations remain paused.
