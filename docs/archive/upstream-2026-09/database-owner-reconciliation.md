> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile remaining database owners

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Read the complete diffs for the eight remaining database rows. Verify each
preserved snapshot hash. This batch changes review records only: no production
Rust, SQL migration, schema version, recorded migration hash or user database.

| Preserved path | Current owner and disposition |
| --- | --- |
| `migrations/0040_chief_capacity_refusal.sql` | Current migration 38, `chief_dispatch_refusals`, retains exact resolved refusal proof for a claimed retry. It retains the old server-draining reason and adds other explicit known-unsent reasons. |
| `migrations/0042_chief_managed_provider_refusal.sql` | The same migration retains managed-provider refusal. Its SQL requires the same work and retry event, an allowed reason and resolved disposition. Tests reject foreign work, wrong retry, unknown reason and unresolved proof, and prohibit replay after cancellation. |
| `src/chief/tests/reasoning_summary.rs` | All original assertions remain. The completion/bounds assertions move into a helper. New checks cover an out-of-range initial part, output-revision notification, and keeping internal voice provenance out of public transcript results. |
| `src/chief_reasoning_summary.rs` | Preserve active work/turn/generation ownership, completion rules, bounded parts and voice provenance. Notify the output revision after a write. Persist truncation even when the first received part exceeds the part limit. Ignored foreign or completed updates do not manufacture output. |
| `src/chief_questions.rs` | Preserve question identity, duplicate conflict checks, answers, skips and recovery filtering. Add first-insertion live-arrival provenance. A later duplicate live event does not upgrade a historical question. Migration and reopen tests retain historical false values. |
| `src/chief_request_payload.rs` | Keep small events inline and large approvals in the dedicated immutable payload table. The envelope can contain two individually bounded native frames plus routing metadata; validate method, matching file item, each frame and metadata before compaction. This does not grant arbitrary events the larger bound. App UI receipts use their own owner. |
| `src/chief_app_exposure_tests.rs` | The exposure-only writer is replaced by the shared App/Hook journal tested in `src/chief_process/tests/app_settings.rs`. Exact review, account/process/connection target, single-use consent and cross-writer exclusion remain. Reopen recovery distinguishes an absent value from an explicit empty list. Historical exposure receipts remain readable through the separately restored legacy reader; no old write is replayed. See [App settings reconciliation](app-settings-owner-reconciliation.md). |
| `src/migrations.rs` | The registry owns the current version-48 sequence. The migration engine and ledger verification are retained. Tests move or adapt to the current predecessor versions and add checks for nullable effort, question arrival, output completion, file envelopes, recap, prompt input and native settings. Initial-model-source and voice-precaution tests move to dedicated modules. |

## Migration interpretation

The preserved branch used different numbers and SQL for some experimental
migrations. The current registered sequence is authoritative. A renamed source
mapping does not mean the old SQL can be replayed over a current database or that
an arbitrary experimental database is supported. Existing ledger checksum and
schema validation remain in force. No historical migration is rewritten here.

The old effort table required non-null values and included SQL control-character
checks. The current nullable table permits native inheritance; its application
write owner rejects control-bearing values. The exact difference and original
request-field preservation are recorded in
[conversation persistence](conversation-persistence-reconciliation.md). Do not
claim identical raw-SQL validation.

Current upgrade tests verify saved request text and identities, retained output,
existing preferences and revisions, unchanged earlier ledger entries, foreign
keys, invalid transitions and repeated migration. Larger native file evidence
uses the current envelope bound; the original smaller limit is not silently
claimed as retained. Optional recap defaults off.

## Validation and limits

Run the database library suite on disposable SQLite fixtures. All 176 tests pass with none ignored in 110.90 seconds. The complete log is
`/tmp/decodex-database-reconciliation-tests.log`.
This closes the eight source mappings only. It does not prove every experimental
schema is upgradeable or qualify production-sized databases and signed desktop
lifecycle behavior. No real user database was opened by this test run.

Persistence identity, no replay, complete evidence and truthful arrival/output
state are core behavior. Connector settings and recap remain optional consumers
for the user's removal review. Automations remain paused.
