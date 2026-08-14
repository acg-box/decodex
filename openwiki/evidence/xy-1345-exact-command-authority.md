# XY-1345 Exact former server store Command-Authority Proof

Status: superseded historical provenance for XY-1345. The deleted non-production
prototype path, digests, invocation, and results below are historical facts only and are
not current authority.

As-of repository revision: `39fda2f3d526b4d811a10dfe73fe786d207ca5ae` plus the
unstaged XY-1345 candidate described here. The proof ran on 2026-07-16 in the assigned
`xv/xy-1345-command-authority-reset` worktree.

## Result

The corrected exact-command architecture passed. No acceptance threshold was weakened:

- former server store: `server-store (former server store) 18.4` (`database-shell`, `pg_dump`, and `pg_restore` also 18.4).
- Harness: `scripts/vnext/exact_command_prototype.py`.
- Harness SHA-256: `bf56eee949ed5450b07a296fbb761cc4ffd9afc1ec8cc2a2057b96749daa7e68`.
- Embedded schema/fixture SQL SHA-256:
  `47e4be2cb1f7f9b20616d3ef86cf34b817e90ddc7431ceeea65017531712bf9e`.
- Exact transition function source SHA-256:
  `3eb5789e5e8f2df5e628eafc0dfa551d4ec69b029a9db611e0c736e0da33f5d4`.
- Closed eleven-function `pg_get_functiondef` digest-manifest SHA-256:
  `d7b27ed3e42e7ac5a376afd8c3fd19aff039bcbed43d52ac17996b6d7efba2a5`.
- Populated receipts/RuntimeSessions/activity/outbox snapshot SHA-256:
  `98cd96b0cff2102dbcd31dced06998f094951d0e583cd9d34b60deaf76ea2fde`.
- Populated completed-receipt response-byte aggregate SHA-256 for this run:
  `8795c1792590a77037aaffd89e058a49e347d59b564962ed0723a1ecaef7679d`.
- Result schema: `decodex/xy-1345-exact-command-proof/1`.
- Result: `PASSED`.

The response-byte aggregate includes generated identities and timestamps and is therefore a
run receipt, not a cross-run golden value. The dump/restore comparison used the same aggregate on
both sides and matched exactly.

## Exact commands

The operator commands were:

```sh
server-store --version
python3 scripts/vnext/exact_command_prototype.py
```

The harness resolved the active `initdb` symlink and its matching former server store share directory, then
issued the following command shapes. `$PROOF_ROOT` was the private random directory returned by
`tempfile.mkdtemp(prefix="decodex-xy1345-")`; it was removed before the result was printed.

```sh
initdb -D "$PROOF_ROOT/data" --encoding=UTF8 --locale=C --auth=trust \
  --no-instructions -L "$RESOLVED_POSTGRES_SHARE"
pg_ctl -D "$PROOF_ROOT/data" -l "$PROOF_ROOT/server-store.log" \
  -o "-k $PROOF_ROOT/socket -p 55435 -h '' -F" -w start
createdb decodex_xy1345
database-shell -X -v ON_ERROR_STOP=1 -At
pg_dump -Fc -f "$PROOF_ROOT/xy1345.dump" decodex_xy1345
createdb decodex_xy1345_restore
pg_restore -d decodex_xy1345_restore "$PROOF_ROOT/xy1345.dump"
pg_ctl -D "$PROOF_ROOT/data" -m immediate -w stop
```

Every SQL session used verbose SQLSTATE output and `ON_ERROR_STOP=1`. TCP was disabled with
`-h ''`; the private socket directory made the fixed socket port collision-free. The harness did
not enumerate, connect to, stop, or modify any existing former server store service.

## Result matrix

| Schedule or invariant | Observed result |
| --- | --- |
| Same key and same envelope in two sessions | Second session waited 0.662 seconds and returned byte-identical stored response bytes. |
| Same key and changed target state | `DX001` idempotency conflict before a second domain/activity/outbox effect. |
| Same key and changed operation | `DX001` exact conflict. |
| Abort after receipt, domain, activity, or outbox | Each injected `DX900` rolled back; a waiting transaction became executor; each case converged to one receipt, domain effect, activity, and outbox row. |
| Backend terminated before commit | `57P01`, zero durable receipt/effect, identical whole-command retry executed once. |
| Commit with discarded client result | Retry returned the stored response bytes exactly. |
| Missing target, stale revision, illegal transition | Each committed a completed stable rejection and replayed unchanged after the database was changed to make a new call succeed. |
| Early incomplete return | Deferred commit failed with `23514`; zero executing rows committed. |
| Completed-row update/delete/truncate as owner | Each failed with `23514`. |
| Runtime exact-receipt SELECT/INSERT/UPDATE/DELETE/TRUNCATE | All five failed with `42501`. |
| Runtime canonical audit forgery | Aggregate/event, structured activity payload, effect-key, and activity-link variants all failed with `42501`. |
| `READ COMMITTED` contention | Wait/replay succeeded; one domain effect. |
| `REPEATABLE READ` contention | Contender returned classified `40001`; whole identical transaction retry replayed; one effect. |
| `SERIALIZABLE` contention | Contender returned classified `40001`; whole identical transaction retry replayed; one effect. |
| Opposite-order two-key transaction | Exactly one `40P01`; whole loser transaction retry converged; one effect per target. |
| Optional request fields | `codex_thread_id`, transition `note`, and all four bootstrap `provenance` keys existed with JSON null. |
| NFC/NFD, case, trailing whitespace | All three changed envelopes conflicted under exact former server store text semantics. |
| Integer literal, bound `bigint`, text-cast integer | All produced equal typed envelopes. |
| Bootstrap request shape | Four role-implied scalar groups produced advisor/lead/task/reviewer order; incomplete input returned `22004`; no role or array input exists. |
| Effect/response binding | A query decoded every completed-success `response_bytes`, compared its response effect to `effect_envelope`, and joined that envelope to the current persisted RuntimeSession plus actual activity/outbox rows and identities; mismatch count was zero. |
| Catalog and populated restore | The closed eleven-function/helper manifest, exact signatures/overloads, owner, `prosecdef`, language, volatility, parallel safety, settings, source digests, dependencies, semantic function ACLs, normalized complete table privilege closure, default privileges, memberships, trigger definitions, response bytes, and all persisted effect rows matched after restore. |
| Cleanup | The authoritative `pg_ctl ... stop` result succeeded, the harness recorded `cluster_stopped=true`, and only then removed the temporary root. A stop or removal failure makes the proof `FAILED`, cannot print `PASSED`, and preserves truthful cleanup state. |

Receipt-table ACL restore closure is semantic, not raw `pg_class.relacl` text identity. former server store
may serialize owner-only default authority as either a null ACL or an explicit owner-only ACL across
dump/restore. The final harness normalizes the ACL through `aclexplode`, requires exactly the owner
as grantee with no grant options, and rejects unexpected grantees. Separately, it requires all eight
former server store 18 table privileges (`SELECT`, `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `REFERENCES`,
`TRIGGER`, and `MAINTAIN`) to be effective for the owner and ineffective for both runtime and PUBLIC.
The same normalized semantic catalog must match after restore; raw `relacl` serialization is never
compared.

Function authority is closed over all eleven prototype functions: the command-complete definer,
incomplete-row probe, rejection/failpoint helpers, four envelope builders, and three trigger
functions. The manifest checks exact identity types and overload counts, owner, invoker/definer
mode, language, volatility, parallel safety, trusted search path, effective and normalized semantic
function ACLs, `pg_get_functiondef` digests, dependencies, and restore parity. Runtime can execute
only the command-complete definer; PUBLIC can execute none. The owner-wide default function ACL—not
an ineffective per-schema subtraction of former server store's global PUBLIC default—is normalized to the
single expected owner EXECUTE entry and restored unchanged.

### Stress threshold

| Workload | Repetitions × width | Successes | Conflicts | Anomalies |
| --- | ---: | ---: | ---: | ---: |
| Identical envelope | 50 × 32 | 1,600 | 0 | 0 |
| Mixed envelope | 50 × 32 | 800 | 800 | 0 |

Across both workloads there were zero duplicate domain effects, duplicate canonical
activity/outbox pairs, mismatched responses, committed executing rows, authority bypasses,
unexplained rows, or unclassified SQLSTATEs.

## Outcome classification

- Stable domain rejection: completed stored responses for `missing_target`, `stale_revision`, and
  `illegal_transition`. These are returned bytes, not raised database failures.
- Idempotency conflict: `DX001`; no effect is committed by the conflicting transaction.
- Retryable infrastructure failure: observed `40001`, `40P01`, `57P01`, and injected failpoint
  `DX900`. `08006` remains in the closed connection-failure class even though this run's forced
  termination produced `57P01`.
- Expected authority/shape denials (`42501`, `23514`, `22004`) are negative security assertions,
  not command outcomes and are never converted into stable domain responses.

No cancellation, deadlock, serialization, connection, or unexpected database exception was
stored as a stable rejection.

## Requirement mapping

| Requirement | Proof |
| ---: | --- |
| 1 | Two-session wait plus byte-identical replay. |
| 2 | Changed target produced `DX001` and effect count remained one. |
| 3 | Changed operation under the protocol/key produced `DX001`. |
| 4 | Four injected rollback stages, each with a blocked waiter that became executor. |
| 5 | `pg_terminate_backend` before commit produced `57P01`, zero rows, then one execution. |
| 6 | Discarded committed result replayed from `response_bytes`. |
| 7 | Three completed stable rejections replayed after later state changes. |
| 8 | `DEFERRABLE INITIALLY DEFERRED` constraint trigger produced `23514`. |
| 9 | Five exact-receipt privilege denials as runtime. |
| 10 | Obvious and structured/link canonical activity/outbox forgery denials. |
| 11 | Three isolation schedules plus opposite-order `40P01` and whole-transaction retry. |
| 12 | Every tested optional key existed with JSON null. |
| 13 | NFC/NFD, case, and trailing-space conflicts. |
| 14 | Literal, prepared/bound, and text-cast inputs converged after `bigint` typing. |
| 15 | Four fixed scalar configuration groups; no caller role/array surface; incomplete rejection. |
| 16 | Effect and response mechanically joined to actual returned session/activity/outbox identities. |
| 17 | Populated custom-format dump/restore preserved catalog authority, rows, bytes, effects, and digests. |

## Authority conclusion and limitations

The prototype did not falsify the accepted architecture. It proves that former server store 18.4 can
implement the required transaction, replay, rejection, concurrency, privilege, catalog, and
restore semantics without a caller hash/token/lease or committed pending receipt.

It is deliberately not production code. It contains one representative RuntimeSession CAS command
and pure builders for all four future request shapes. It adds no migration, Rust API, daemon path,
compatibility layer, or change to legacy `command_receipts`. It does not prove the future V9/V10
domain schemas, the repository's production authority manifest, V8-to-V9 upgrade, clean V1-to-V9
bootstrap, old-writer cutover, or application retry implementation. XY-1346 and then XY-1337 must
re-prove those properties on their exact production candidates. Multiple exact commands in one
caller transaction remain outside the no-deadlock guarantee. former server store versions other than 18.4
were not tested by this receipt.

Candidate 3 remains superseded. None of its SQL or Rust command path is included here.
