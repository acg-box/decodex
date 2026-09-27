# Preserve automatic fallback identity across journal upgrades

The legacy automatic recovery journal uses `model-recovery:<digest>`. The current
shared model journal uses `model-selection:<digest>`. Both derive the digest from
the same ordered work, thread, account, banner, source model, target model, effort
and service tier. Process generation, account revision and read timestamps do not
create a new automatic occurrence. The two original writer and producer files
were compared with the current owners to verify this identity contract.

Pending legacy operations already block the current writer. Terminal legacy
operations do not remain pending, so that guard alone cannot preserve single use.
The current writer could reserve the same occurrence under its new prefix after a
legacy rejection, target observation or restart reconciliation.

Check the corresponding legacy key inside the existing immediate reservation
transaction before inserting a current automatic attempt. Any original reservation
consumes the occurrence regardless of its later result. Keep both journal formats,
original payloads and current stable keys unchanged. Manual requests keep their
existing path. No migration, replay worker or extra journal is added.

A disposable SQLite regression retains a legacy reservation and terminal result,
reopens the database and verifies that no operation is pending. Before the fix,
the current reservation incorrectly succeeds after a legacy rejection. The fixed
test covers rejected, observed and reconciled records. It also verifies that a
different banner digest can reserve a new occurrence.

Failure evidence: `/tmp/decodex-legacy-fallback-identity-before-qualified.log`.
This restores the no-replay contract for the optional automatic fallback policy.
It closes no additional inherited whole-file rows and does not change live user
state, installed binaries or maintenance automations.

All 22 database model tests pass, as does strict database Clippy with all features
and targets. Logs: `/tmp/decodex-legacy-fallback-identity-after.log` and
`/tmp/decodex-legacy-fallback-identity-clippy.log`.
