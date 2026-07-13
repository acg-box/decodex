# XY-1262 Codex runtime proof

Status: gate evidence at repository revision `f9d6c4e70198e94e5b9461b8cac7518ae14d41ef`.

Observed: 2026-07-13 using `codex-cli 0.144.0-alpha.4` and the normal shared
`~/.codex`. This is a proof spike, not the vNext adapter.

## Reproduce

The probe reads two explicitly selected records from the existing Decodex account pool,
passes their tokens only through process-scoped `account/login/start`, redacts account
identity as A/B, and verifies that `~/.codex/auth.json` has the same SHA-256 before and
after. It does not select, import, remove, or overwrite accounts or plugins.

```sh
python3 scripts/vnext/codex_app_server_probe.py schema
python3 scripts/vnext/codex_app_server_probe.py live \
  --account-a <selector-a> --account-b <selector-b>
python3 scripts/vnext/codex_app_server_probe.py validate
python3 -m unittest tests/scripts/test_codex_app_server_probe.py
```

The checked-in redacted live receipt is
[`fixtures/xy-1262-live-receipt.json`](fixtures/xy-1262-live-receipt.json). The typed
quota cases are
[`fixtures/xy-1262-quota-matrix.json`](fixtures/xy-1262-quota-matrix.json).
The native reviewer event is
[`fixtures/xy-1262-native-collaboration.json`](fixtures/xy-1262-native-collaboration.json).
The live fixture is an evidence bundle, not raw probe stdout: its `sources` array records
the probe, Codex Desktop, and focused archive/ownership readbacks that were normalized
into it. `validate` rejects credential-shaped fields and checks the bundle invariants.

## Results and falsifiers

| Gate | Verdict | Direct evidence | Falsifier / downstream rule |
| --- | --- | --- | --- |
| Shared-home persistence | Pass | A non-ephemeral, named thread was created at the repository cwd, received a completed turn, survived a killed runner, and was returned by `thread/list(searchTerm=...)` and `thread/read(includeTurns=true)` from a new process. Codex Desktop `read_thread` returned both turns by exact ID. | Fail if a normal app restart loses the rollout or the app cannot read the exact ID. |
| Search/UI discovery | Partial fail | App-server filtered list found the title after restart and Codex Desktop could read the thread, but `thread/search` and Codex Desktop global query did not return it in this run. | XY-1270/1272 must not equate app-server listability with sidebar/global-search discovery. Keep this M0 sub-gate failed until a desktop restart/indexing experiment returns the thread by title. |
| Ownership isolation | Partial pass | The only mapped IDs are IDs returned to the probe's own `thread/start`; the same cwd listing contained other threads, all counted and ignored without reading their turns. A production mapping transaction does not yet exist. | Never discover ownership from cwd, source, title, list membership, or rollout scanning. Persist the creation receipt transactionally before exposing the Conversation. |
| External activity/divergence | Partial pass | A later turn was sent through Codex Desktop rather than the probe runner and became observable by `thread/read`, proving the last-known-turn mismatch input. `threadSource` also disappeared after resume while visible messages remained. No production ManagedRun exists to execute the `diverged` transition. | Ordinary Conversation reconciliation may provenance-import visible messages. Any unseen turn on an active ManagedRun sets `RuntimeSession=diverged` and blocks side effects until receipts, Git/worktree state, and artifacts reconcile. Never infer tool completion from `thread/read`. |
| Schema/capability negotiation | Pass with contradiction | Canonical-JSON schema digests expose start/list/search/read/resume, quota, and collaboration shapes. Raw aggregate schema bytes are nondeterministic across generations, so the probe canonicalizes JSON before hashing. A live `historyMode=paginated` start returned JSON-RPC `-32601` (`paginated_threads` unsupported); retry with `legacy` succeeded. | Generated schema is not a runtime capability promise. XY-1270 must negotiate initialize/schema plus live method outcome and cache the observed capability by CLI build. |
| Real cross-account continuation | Partial pass; quota-failover gate failed | Account A created and completed a turn; its process was killed; a different stored/authenticated Account B resumed the exact thread ID and completed a second turn. Both accounts had fresh 7-day windows at 0% used, so this did not exercise quota exclusion or depletion. | Same-thread cross-account resume is proven only for this healthy account/build pair. XY-1273 remains blocked on a real quota-failure exclusion/failover run. |
| Context-Pack/new-session fallback | Partial pass; denied-resume gate failed | A process-scoped invalid credential was rejected before resume/turn. A new persistent RuntimeSession on healthy Account A received a Context Pack naming the prior session, durable marker, repository HEAD, and `possible_side_effects=none`, then completed. No same-thread resume rejection/incompatibility occurred. | The mechanism is viable, but XY-1271/1273 still require a real resume-denied/incompatible path. The fallback must preserve logical IDs and never replay a possibly side-effecting turn. |
| Explicit archive | Pass | The probe archived only its fallback thread, observed it through `thread/list(archived=true)`, and unarchived it. The primary meaningful proof thread remained unarchived. | vNext must remove current terminal auto-archive behavior; retention is an explicit policy command with readback. |
| Native collaboration | Pass for observed run-local shape | The mandatory reviewer produced a child thread returned by `thread/list(ancestorThreadId=...)` with `parentThreadId`; Codex Desktop readback showed a completed child turn containing `subAgentActivity { kind=interacted, agentThreadId=<parent>, agentPath=/root }`. Schema additionally exposes `collabAgentToolCall`, nickname, and role fields. | Normalize observed parent/child, activity, correlation, and terminal fields without treating optional nickname/role as identity. Native spawn remains run-local, never the durable cross-account/top-level router. |

## Crash and recovery matrix

| Crash/failure point | Durable fact required before crash | Recovery result | Duplicate/side-effect rule |
| --- | --- | --- | --- |
| Pre-submit selection, before exclusion commit | No turn ID or submission receipt | Repeat selection/probe; no account is yet excluded. | This row applies only before turn submission. |
| Rate limit observed after turn submission, before exclusion commit | Turn ID, error/limit observation, and side-effect state `unknown` | Reconcile the old turn, receipts, Git/worktree, and artifacts; then persist the specific account/window exclusion before selecting a fallback. | Never infer that no side effect occurred and never replay blindly. |
| After exclusion, before runner start | Account/window exclusion with observation and reset | Select another eligible account or persist `waiting_usage`. | No turn was submitted. |
| Runner dies after `thread/start` response | Conversation/RuntimeSession/thread mapping and creation receipt | `thread/list`/`thread/read`; adopt the one mapped thread. | Never start a replacement until readback proves mapping state. |
| Runner dies after `turn/start` but before terminal event | Turn ID plus submitted idempotency key; side-effect state unknown | Resume/read, then reconcile tool receipts, Git/worktree, and artifacts. | Never replay the turn blindly. |
| Account B login rejected | Typed `auth_failed` exclusion before fallback | Choose another available account; otherwise typed auth wait. | No resume or turn on the rejected account. |
| Same-thread resume rejected/incompatible | Failed RuntimeSession and prior last-known turn | Start one new RuntimeSession from a Context Pack. | Preserve Conversation/ManagedRun; do not forge the old thread mapping. |
| All accounts have fresh depleted windows | Per-account depleted windows | `waiting_usage`; wake at `min(max(account depleted resets))`. | Unknown/stale/auth-failed accounts are not silently treated as depleted. |

The live process-kill experiment covers runner death and shared-rollout persistence. The
remaining broker crash rows are executable requirements for XY-1273/1274 rather than a
claim that a production broker already exists.

## Quota contract

Window class is determined only by `windowDurationMins`: 300 is five-hour and 10080 is
seven-day. `primary`/`secondary` are retained solely as source provenance. Missing or
stale windows and elapsed resets transition to `unknown` and require a bounded fresh
probe. A fresh zero remainder excludes an account until that account's latest depleted
window reset. If every account is excluded only by usage, the ManagedRun waits until the
earliest per-account ready time. Authentication failure is a separate exclusion and has
no quota reset time.

The live accounts exposed 7-day windows only. Therefore the live 5-hour observation is
`unknown`; no 5-hour state is inferred from the positional fields. The typed fixture
covers available, five-hour depleted, seven-day depleted, unknown, stale, reversed,
reset elapsed, auth-failed, and all-accounts-depleted behavior.

## Adapter implementation conclusions

1. XY-1270 must pin generated-schema digests and separately record live capability
   outcomes. It must support legacy history for this build and treat paginated history
   as unavailable despite its schema enum.
2. XY-1271/1272 must persist Conversation, RuntimeSession, owned creation receipt,
   account/profile snapshot, thread ID, and last-known turn. `thread/read` is visible
   message reconciliation, not a complete tool/side-effect ledger.
3. XY-1273 must bind one runner process to one account and never overwrite normal
   `auth.json` for routing. Same-thread cross-account continuation is build/capability
   gated; fallback creates one new RuntimeSession from an inspectable Context Pack.
4. XY-1274 must store typed window duration, remaining amount, reset, observation time,
   and confidence. It must never derive five-hour/seven-day identity from
   primary/secondary position.
5. XY-1280 collaboration normalization must preserve parent thread, native actor,
   tool-call correlation, status transitions, and terminal outcome. Durable routing
   remains Decodex-owned.

Acceptance verdict: **failed**. Healthy-account same-thread continuation, Context-Pack
mechanics, ownership-by-creation-receipt, exact-ID readback, native run-local
collaboration shape, archive behavior, schema negotiation, and the typed quota decision
table are usable downstream evidence. Global title discovery, real quota-depletion
failover/exclusion persistence, a resume-denied Context-Pack transition, and an
executable ManagedRun divergence transition remain unproven falsifiers.

## Independent review

Reviewer `/root/xy1262_reviewer` performed read-only review over the actual diff and
evidence. The first review rejected quota-failover, denied-resume fallback, external
divergence, native collaboration, receipt reproducibility, credential safety, and one
crash rule as overclaimed or incomplete. All valid findings were dispositioned by
downgrading the unrun gates, adding external/native readbacks, canonical schema hashing,
an executable input-only quota classifier, redaction validation, auth-integrity failure,
and separate pre-/post-submit crash rules. After a material re-review and a targeted
final confirmation, the reviewer reported no remaining blocker and accepted this as a
focused failed-gate evidence commit, not as a passing XY-1262 gate.

## Gate reconciliation follow-up

Observed 2026-07-13 from merged evidence revision
`fc110cbcbdd1fd33187f597536c3faada6eb6cbc`. The redacted receipt is
[`fixtures/xy-1262-gate-reconciliation.json`](fixtures/xy-1262-gate-reconciliation.json).
The inventory authenticated all six configured Decodex account records through separate
process-scoped app-server children without starting a turn. It recorded only opaque
aliases and duration-typed quota windows. All six returned usable plugin/skill
inventories with no scan/load errors and permitted a no-turn resume of the retained
proof thread. Normal `auth.json`, the non-transient plugin tree, and the Decodex account
pool hashed identically before and after; each process also returned the same plugin
inventory counts. The app-server executable cache refreshed on the first broad tree
check; it is transient process machinery and was excluded from the plugin-tree integrity
scope. The receipt is a normalized multi-source bundle: the inventory command supplied
account/app-server facts and the supported Codex Desktop read supplied the global-title
result.

No account had both duration classes and no observed duration-typed window was depleted.
Every account therefore remained `unknown`, not `available` or `depleted`; no turn was
submitted and no quota was deliberately consumed. There was consequently no honest way
to exercise provider quota failure, exclusion-before-fallback persistence, crash
readback, or selection/continuation on a different eligible account. Likewise, every
safe account permitted same-thread resume, so no genuine denied/incompatible boundary
existed for a Context-Pack transition.

Delayed discovery also remained split: app-server `thread/list(searchTerm=...)` found
the retained title, while app-server `thread/search` and a supported Codex Desktop
global title query returned no match without restarting or mutating Codex Desktop or the
Manager task. The visibility sub-gate remains failed.

### Proposed gate split — not accepted authority

The current [gate manifest](../specs/vnext-gates.md) still makes the complete XY-1262
gate an M0 prerequisite and says a failing owning gate freezes the affected milestone.
Nothing in this follow-up changes that authority. The following is a repository-owned
proposal for explicit Manager acceptance and a corresponding normative manifest edit:

1. Define an **XY-1262 foundation gate** from already observed evidence. It permits
   shared-home and one-account-per-process boundaries; creation-receipt ownership;
   typed schema plus live capability negotiation; exact-ID/list/read/archive behavior;
   lossy-read/divergence policy; native run-local collaboration normalization;
   process-scoped auth and redaction; read-only plugin/skill inventory; and pure quota
   policy keyed only by duration 300/10080.
2. Allow M1 foundations after their own M0 gates pass: workspace owners (XY-1265),
   loopback protocol/idempotency/reconnect foundations (XY-1266), PostgreSQL
   transactions/leases/outbox and inert account/window schemas (XY-1267), owned paths
   plus API-only diagnostics (XY-1268), and the GPUI shell after the separate GPUI gate
   (XY-1269). These surfaces must expose unavailable/unknown states honestly.
3. Allow bounded M2 foundations, without enabling routing: generated typed app-server
   contracts and process supervision (XY-1270); Conversation/RuntimeSession/history and
   inspectable Context-Pack persistence (XY-1271); transactional creation mappings,
   exact-ID reconciliation, explicit retention, and the ManagedRun `diverged` stop
   transition (XY-1272); credential-vault metadata and immutable runner binding without
   automatic assignment (XY-1273); pure duration-typed quota/wake calculations and
   durable exclusion transaction tests with synthetic fixtures only (XY-1274); and
   user-owned profiles plus read-only readiness audits (XY-1275).
4. Keep **all live account-routing features disabled by default**: sticky or policy
   account selection, quota-triggered exclusion driving another assignment,
   `waiting_usage` scheduling/wakeup, automatic cross-account same-thread resume,
   automatic Context-Pack fallback, and any replay after uncertain side effects. Keep
   XY-1276 Quick Task failover/release acceptance blocked. Do not claim global Codex
   title discovery; exact-ID/list visibility is the only proven contract.
5. Create a later **XY-1262 live enablement gate**. On a naturally depleted configured
   account, a fixed no-tool marker must return a typed provider quota failure. Durable
   readback must then show the submitted turn and unknown side-effect state, followed by
   the specific 300/10080 account/window exclusion committed and crash-recoverable
   before any fallback assignment. A different fresh eligible account must produce
   exactly one useful continuation on the same thread when permitted; otherwise a real
   denied/incompatible response must end the old RuntimeSession and create exactly one
   Context-Pack RuntimeSession. After an injected crash at each boundary, readback must
   show one continuation, correct `waiting_usage`/ready time when applicable, and no
   duplicate tool, worktree, Git, or artifact effect. Auth, account-pool, and installed/
   enabled plugin state must remain unchanged. Separately, the retained title must be
   returned by supported Codex Desktop discovery after normal indexing before the
   visibility claim or XY-1276 acceptance is enabled.

Continuity classification: `same_decision_changed_context`. The current authority,
merged XY-1262 evidence, its Linear issue/comments, the accepted design baseline, and
XY-1265 through XY-1276 scopes were checked. Newly observed evidence strengthens safe
process isolation and inventory coverage from two selected accounts to all six
configured accounts, but it does not close any failed live gate. Decision impact remains
blocked under current authority; authority action is `propose_update`, not implicit
supersession. The proposal is falsified if the foundation surfaces cannot remain
mechanically disabled from live routing, or if later natural depletion shows that the
transaction ordering or continuity model cannot produce exactly one safe continuation.
