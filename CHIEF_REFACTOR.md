# Chief Refactor

## Accepted outcome

Build a personal Chief that coordinates multiple goals, independent Codex threads,
and automation results. Decodex owns work relationships, communication, follow-up,
and durable dispositions. Codex app-server owns model and tool execution. Native
subagents are not a dependency of this product design.

## Delivery sequence

### Final execution sequence

1. Verify the complete user loop through the account-owned service and native UI:
   delegation, worker completion, original-thread repair, automation intake, due
   follow-up, user decision, and restart. Make conversation and decisions primary.
2. After that proof, remove active Factory/Program/mandatory Review entrypoints
   across UI, protocol, runtime and database writers. Keep historical data, readers,
   ordinary conversations and account/app-server safety. Do not merely hide tabs.
3. Validate the combined source, migration compatibility and native interactions;
   then perform authorized delivery. Keep unrelated icon/packaging work separate.

The integration owner is this task. Existing internal workers own exclusive UI,
service qualification and retirement scopes. No additional sidebar tasks are used.

Implementation acceptance is complete. Repository delivery is tracked by the PR.
Found and repaired a missing user-decision reply path: a current
Chief user message can resolve the exact idle decision work item, with a new causal
receipt and unchanged original evidence. It cannot grant provider execution approval.
The targeted regression test and the full account-admitted service loop passed.
Native ComposerInput Enter also sent a real user message, cleared only after
acceptance, and displayed the actual `UI_READY` reply above the fold. Evidence is
in `target/visual-tests/chief-conversation-send.{png,send.json,evidence.json}`.

Stage 2 completed: Factory presentation and controller paths are deleted. The
four Program mutation commands, mutation DTOs, execution admission and database
writers are removed. Historical queries, conversation lineage, released migrations
and records remain readable. Chief is the default primary destination.
Integration passed the combined verification below. The branch incorporates remote
main through `d093c5f203d3a644482f590062365077db0897a1`; unrelated icon work is preserved.

Final acceptance:

- `cargo make test-rust`: 1166 passed, 11 skipped, all workspace targets/features.
- `scripts/lint_rust_workspace.py`: all 12 packages passed the strict deny flags.
- `cargo make fmt-check`: Rust and TOML formatting passed.
- Architecture and gate-contract Python suites: 26 passed.
- CLI diagnostic process tests: 2 passed.
- SQLite schema-14 gate and historical upgrade/readback tests passed.
- Native quota tests: 44 passed; native Chief Enter-to-real-response proof retained.
- Explicit goal resolution requires current related evidence; worker completion
  alone leaves the parent goal open. The regression test passed.

No checks were disabled to obtain acceptance. The stale nextest `decodexd` binary
selector now names the current CLI shutdown tests and retains its serial group.

| Stage | Deliverable | Acceptance | State |
| --- | --- | --- | --- |
| 1 | Multiplexed app-server connection | Two independent threads; one turn ending does not close the connection; inbound requests remain addressable | Multiplexing verified; account-owned service start and restart verified |
| 2 | Durable work graph and inbox | Restart preserves goals, bindings, dependencies, unprocessed events and dispositions; duplicates do not create work | Foundation verified |
| 3 | Chief coordination | Dispatch, receive, resume the original Chief and continue the original worker; actual model and permissions | Isolated worker/repair smoke passed; admitted Chief start and same-thread restart verified |
| 4 | Follow-up and automation intake | One real input source, due checks, batch handling and evidence-backed briefings | Real service intake, deduplication and scheduled check across restart verified |
| 5 | GPUI work surface | Chief conversation, briefings, decisions and a derived work graph operate on real data | Native Enter send and actual response verified; conversation-first layout accepted |
| 6 | Cutover and qualification | Preserve existing data and account behavior; remove replaced harness; restart and live execution acceptance | Active old paths retired; combined acceptance passed; PR tracks delivery |

Stages are dependencies, not separate product managers or mandatory model roles.
This file tracks unfinished implementation across context changes. Passing an
isolated stage does not mark the whole refactor complete.

## Scope and boundaries

- Retain the current Rust service, SQLite authority and GPUI client.
- Use independent Codex threads. Do not assume a thread equals a process or worktree.
- The model chooses decomposition, ordinary repairs and acceptance judgment.
- Code owns message correlation, actual execution policy, persistence and wakeups.
- Keep uncertain external dispatch visible; do not retry it blindly.
- Existing account credentials, data and released migrations must remain intact.
- Preserve the untracked `work/chief-architecture-review.html` design artifact.
- Do not revive QuickTask, static Factory personas, or mandatory review cycles.
- Keep one integration branch. Workers have separate file scopes; integration owns Git.

## Model adaptation

The 2026-09-11 OpenAI article recommends narrow skill descriptions, progressive
disclosure, fewer procedural recipes and explicit task completion boundaries.
Apply this to Chief instructions: state its responsibility, available tools,
delegation scope and completion criteria. Do not prescribe a fixed planning and
review sequence. Execution workers default to the selected model with medium effort;
capability readback must confirm the actual configuration.

Source: https://developers.openai.com/blog/rethinking-skills-and-prompts-for-gpt-6-astra

## Required end-to-end evidence

Two workers execute independently. The Chief finishes its initial turn. Worker
completion reaches a persistent inbox and causes a later Chief turn. One result
requires repair in the original worker; one automation result is deduplicated;
one decision is surfaced to the user. Restart preserves pending obligations and
does not duplicate uncertain effects. The UI shows the same identities and facts.

## Baseline

Started from `851760f72da1d986d235b43fa21984d7de5a31ba` on 2026-09-13.
Installed Codex: `0.154.0-alpha.6.2`. Protocol compatibility and actual multi-thread
execution must be tested against this installed version, not inferred from docs.

## Historical discussion review

Reference task: `019ff90c-6eb7-7c71-ad26-13320c81b5b4`, "了解 gpui 功能进展".
Reviewed on 2026-09-13. Current conversation and accepted Chief architecture take
precedence. Native pagination failed on the oldest page; the exact task's local
record supplied the earlier user messages and relevant engineering discussion.

Carry forward these lessons:

- A thread can span implementation, checking and repair. Phases are not new jobs.
- Working folders and host Project assignment are execution context, not goals.
  Never infer native Project assignment from cwd or write private host UI state.
- Preserve account affinity and actual quota observations. Unknown quota is not zero.
- Show local history promptly, retain unsent input, and combine stream chunks by turn.
- Keep graph, timeline, conversation and inspector selection on the same identity.
- Collapsible panels, consistent material and visible action feedback are functional
  requirements; test transition frames, not only fully open/closed screenshots.
- A missing lossy history result does not prove that an external action never ran.
  Reconcile uncertain results without replaying the original action blindly.
- Optional or unchecked capabilities must not make healthy core behavior look offline.
- Use real end-to-end dogfood and restart evidence; fixture screenshots do not prove
  working conversations or accurate external state.
- Graph relations should explain actual dependencies and result provenance. Do not
  recreate Git or DeltaDB merely to retain links between work, threads and artifacts.

Do not restore the historical PostgreSQL/redb split, QuickTask public APIs, fixed
Program/Signal/Claim/Review admission ceremony, mandatory reviewer personas,
Linear-owned work state, or per-turn process retirement. New source replaces only
the owners it actually covers, and released data remains readable.

Historical trading, Wasm, extension SDK and multi-machine proposals are references,
not authorization to add those products to this refactor. Generality comes from
small work/result/context boundaries and optional adapters.

## Evidence log

- 2026-09-13: New multiplexed transport fake-server tests passed. Coverage includes
  interleaved thread events, out-of-order RPC replies, explicit approval responses,
  EOF handling and bounded event overflow.
- 2026-09-13: Installed Codex `0.154.0-alpha.6.2`, selected `gpt-6-astra`:
  `real_two_thread_smoke` passed (6.26 seconds). Two ephemeral read-only independent
  threads completed small prompts over one connection; subsequent thread/read
  succeeded. This proves basic multiplexing, not durable Chief wakeup or a precise
  period of overlapping model computation.
- Historical reference task was archived after review. No historical product
  design was promoted over current instructions.
- 2026-09-13: All 32 database tests passed, including schema-12 upgrade preservation,
  opaque thread binding, inbox deduplication, dependency cycles and restart fences.
  The local database gate passed: schema 13, 45 tables, 13 verified migration digests.
- 2026-09-13: `cargo run -p decodex-runtime --example chief_smoke -- gpt-6-astra
  /Users/x/code/acg-box/decodex` passed against installed Codex. A Chief initial turn
  finished, two independent worker threads then ran, completion events woke the
  original Chief and its tools disposed the worker events. Reopening the disposable
  database preserved all three exact thread bindings and work state. No native
  subagent was used. The smoke does not prove crash-time recovery, production
  account integration, GPUI, or scheduled follow-up.
- Live qualification found and fixed a real protocol assumption: a newly started
  thread cannot always be resumed before its first rollout exists. Loaded threads
  now bypass resume; existing unloaded threads resume before a new turn.

## Verified integration boundary

The coordinator is composed into the existing service actor and account-bound
process owner. GPUI and CLI use typed commands and bounded queries through the
same-UID protocol. Live account-bound qualification and native composer send have
passed on disposable profiles. This does not prove an installed upgrade. Do not open the
live database or bypass account/process ownership to obtain passing evidence.

Exact integration seams inspected:

- `account_launch/process.rs`: initialized `AttestedProcessChild` holds private
  stdout pump, request sequence, zeroizing credential callback and process owner.
  Transfer communication ownership without duplicating the account refresh path.
- `process_supervisor.rs`: retain and revoke the shared connection while keeping
  the existing account process admission. The connection must not outlive its
  supervised authority.
- Generic app-server events must not publish account-refresh credential payloads.
- Do not add a second service or direct live SQLite client to bypass these seams.

The signed-process failures were traced to a universal system executable whose
default static architecture differs from its actual runtime architecture. Admission
now compares against the verified architecture identities and still requires exact
runtime hash, path and signature evidence. The runtime library suite passed after
the repair: 268 passed, 1 ignored. This is not installed-application evidence.

Fresh integration checks on 2026-09-13:

- Database, protocol, Codex transport and CLI all-target suites passed (240 tests;
  2 opt-in live tests ignored).
- Local database gate passed: schema 13, 47 tables, 13 migration digests.
- Architecture script suite passed: 15 tests.
- GPUI main suite passed: 143 tests, 5 ignored, before final visual acceptance.
- Host command uncertainty now maps to the existing protocol acceptance-unknown
  result. Start and Send acknowledge durable input independently of process attach;
  attach failures remain visible attention events. Subsequent service and native
  composer qualification verified this acceptance path.
- Extended isolated live smoke passed with `gpt-6-astra`: three independent
  threads, Chief-directed repair in the original worker, one automation receipt
  across duplicate delivery before and after disposition, and a persistent user
  decision. Work and events survived database reopen. All test threads were
  archived and the test-owned process stopped. This still does not prove the
  account-admitted service path or crash-time recovery.

## Initial account-admitted diagnostic (resolved)

The final account-admitted diagnostic used a disposable service and the supported
account enrollment, observation refresh and read APIs. The observer returned a
current profile and an available quota inventory. It recorded a fresh seven-day
quota fact (8% used), but no five-hour fact. Service shutdown completed in 939 ms.
No model turn was submitted in this diagnostic.

That failure was a missing decoded five-hour observation, not an absent observer
or a general provider outage. The old implementation incorrectly required both
windows. The following user clarification and migration resolved that conflict
without a synthetic quota fact or an account-owner bypass.

User clarification: the five-hour window is optional; some accounts do not have
this limit. Implement a distinct `NotApplicable` observation based on successful
provider evidence, not account-plan names. A failed query or an unobserved window
must not become absence. Continue to block known current exhaustion and preserve
account credentials, affinity, process admission and runtime permissions.

Quota transition implemented: `database/` remains the versioned-first owner. New
migration 0014 preserves existing quota rows and adds an explicit absent-window
representation. Released migrations stay unchanged. Test fresh initialization,
schema-13 upgrade, observation ordering, stale absence, and service readback on
disposable databases. No installed database is in scope for this validation.

2026-09-14 optional-window verification:

- The local database gate passed at schema 14 with 47 tables and 14 migration digests.
- Runtime, database, core, protocol and CLI library suites passed: 518 tests,
  1 ignored, before final routing-specific test additions.
- 44 scoped Swift tests and the GPUI quota presentation test passed. The UI shows
  `Not applicable` without a fabricated usage percentage or reset time.
- Only a successful inventory with a valid weekly fact and absent five-hour slot
  can establish absence. Failed, malformed or entirely missing observations cannot.
- Absence uses the original observation timestamp, expires to unknown, survives
  restart, and can replace an older five-hour limit. Current exhaustion still blocks.
- Account-admitted live verification passed: real five-hour `NotApplicable` and
  current weekly usage, `SERVICE_READY` returned through the same-UID history API,
  clean service shutdown, then `SERVICE_RESTORED` on the same exact Codex thread
  after reopening the disposable profile. No directory or authentication gate changed.
- Final library regression run passed 521 tests, with 1 ignored.
- `target/visual-tests/chief-service-live.png` and its `.evidence.json` record the
  populated real service. Assistant output is present in protocol evidence but lies
  below the initial screenshot fold. Full native interaction acceptance remains open.

Latest all-target tests across runtime, database, protocol, transport and CLI:
547 passed, 5 ignored. Chief tests passed in both native main and capture targets.
These counts describe an earlier validation run. Final source validation includes
removal of old active paths and subsequent gate repairs; see the execution status
at the start of this document. No broad lint suppression was added.

Native artifacts under `target/visual-tests/`:

- `chief-initial.png`: unconfigured composer layout, not a connected conversation.
- `chief-service.png` and `chief-service.evidence.json`: actual service snapshot
  with zero work items. The snapshot is real; it does not prove model execution.

No installed profile has been migrated or replaced by this task. Active workflow
cutover and combined acceptance are complete. Repository merge is a separate
delivery fact recorded in the PR and final task response; installation is not
implied by source acceptance. The existing user design artifact in `work/` is retained.
