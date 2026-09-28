> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Chief integration record: current status

The original integration and qualification record is retained below without edits.
It describes successive historical stages, not the current protocol or database
version. Current source uses local protocol 2.95 and schema 48. Do not apply the
old migration numbers or infer current behavior from an intermediate WIP label.

Task-default and current-turn model controls are implemented. Their current owners
and evidence are documented in [task model reconciliation](task-model-panel-reconciliation.md),
[live model control](live-model-control.md), and [model journal reconciliation](model-owner-reconciliation.md).
The older native spawn-description failure was superseded by the qualification in
the live-model record. In contrast, the old child MCP handoff success is contradicted
by the newer [installed-native qualification](native-child-mcp-qualification.md).
Keep that failure visible; no local fallback or relaxed assertion resolves it.

Native collaboration, captured settings, catalog ownership and Guardian execution
remain native responsibilities. The historical limits for cross-client, legacy,
security and signed desktop acceptance remain limits unless a later dedicated
record supplies matching evidence. The shared signed artifact is stale for later
repairs. No complete acceptance or public release is claimed here.

See [the overview reconciliation](overview-record-reconciliation.md) for the full
scope mapping. Automations remain paused, including after completion.

## Original historical record

# Chief workspace integration

## Native collaboration mode on resume

Upstream `91d54f1667e627538db9d44d2ce88a260b4213b0` restores the latest
matching persisted collaboration mode, with a legacy turn-context fallback.
The restoration is retained at cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Decodex accepts the optional resume field and leaves native mode instructions
under native ownership. Chief hydration does not resend creation defaults.

The installed-native recovery fixture now checks Plan and Default, each with
both service-tier update variants. Cold resume preserves mode, custom developer
instructions, recovered model, effort and permissions. The next explicit input
keeps the instruction and recovered model; hydration makes no inference request.
All four cases pass in `/tmp/decodex-1286-native-final.log`.
This does not qualify legacy history, another client's mode change, or a desktop
mode selector. The subsequent runtime cleanup resolves the strict lint failures
reported in `/tmp/decodex-1286-lint-final.log`; current strict all-feature,
all-target runtime lint passes in `/tmp/decodex-runtime-lint-cleanup-verified.log`.

## Scope

Integrate the 49 Chief workspace commits with upstream main `cc44cda4d`.
Keep native paginated history, provider observations, capacity retry and cancellation,
recursive managers, per-message execution settings, attachments, usage, and voice.
The exact-current protocol is 2.26. Older clients must reconnect with a matching build.

## Database upgrade paths

The SQLite owner remains `database/`. SQL migration contents and existing ledger
rows are unchanged. New databases use the local 1–22 sequence, then observation
indexes and capacity retry at 23–24. Databases that already have the upstream
observation migration at 15 retain upstream 15–16, then apply the local additions
at 17–24. Both paths finish at version 24 with the same verified schema inventory.
Selection uses the recorded migration name and still validates every SQL digest.
Unknown or modified histories remain rejected.

Tests reconstruct every version from 14 through 24 in both sequences, preserve the
existing version/name/digest rows, verify final schema parity, and reopen twice.
No user database was reset or changed to resolve the merge.

## Interaction and history

Keep the current continuous Markdown UI. Add capacity cancellation to this UI instead
of restoring the old history cards. Preserve terminal failure text and deduplicate
asynchronous questions against terminal readback. Provider usage observations and
the structured composer/turn usage projection both receive native notifications.

## Limits

The latest physical scrolling and navigation acceptance remains pending. The composer
uses a non-transparent material; true local backdrop blur is not implemented. Existing
opt-in real subscription voice tests are not rerun without their qualification setup.
## Freeform async message qualification

Upstream `b979d4f1f04538ba5a5fcc434d499c007bfe1b8c` adds the native
`send_message_to_user_async` feature opt-in. It is disabled by default. A model
catalog opt-in can also expose the tool; native subagents cannot use it. The
retired `send_async_message` flag does not enable it. Decodex does not force
experimental availability or register a replacement tool.

Installed Codex 0.155.0-alpha.16 passes three isolated local-provider cases:
new flag disabled, retired flag enabled, and new flag enabled with a model absent
from the catalog. Only the last exposes the tool. Its message arrives as an
async agent item without questions, and the same turn continues through a second
model request before completion. Evidence: `/tmp/decodex-1230-native.py` and
`/tmp/decodex-1230-native.log`. These fixtures do not contact the live model service.

The runtime regression now includes duplicate freeform async items with
`phase: final_answer`. It verifies one observation per item, no extra question,
no completion, and no manager wake. The test passes in
`/tmp/decodex-1230-runtime.log`. Signed desktop and combined cross-client
acceptance remain open.


## Captured step settings qualification

Upstream `16537b20a5ec0ea9aa079f4ad4b0e30e8a9efacf` makes request metadata and
tool hooks use settings captured for the issuing step. The native engine owns
this behavior. At cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`, model,
effort, and review policy attribution still use this source.

The installed alpha.16 probe `/tmp/decodex-1241-native.py` holds the first model
request, updates the exact live turn, and permits a second request after a tool
result. `/tmp/decodex-1241-native-current-catalog.log` passes: request models and
metadata change together from `gpt-5.6-sol` / `low` to `gpt-5.6-terra` / `high`.
The probe uses a local backend and no real credentials. It does not test MCP or
hook execution, cold restart, or desktop interaction.

Decodex admits bounded reviewer and model/effort live settings updates. The model
and effort desktop controls are connected; full native and signed acceptance remain open. Connect them through the native exact-turn
API, bind results to the current process and turn, and distinguish publication
from a later observed inference. Do not change future defaults or retry an
uncertain publication. Per-message settings are a separate existing capability.

Upstream `0dfb28edb9305fcae4ab006fb6b7b196cbdbac28` also adds an explicit
session-only model/effort selection. The TUI applies it to the active native
thread and preserves saved defaults, including separate Plan defaults. It
restores Plan effort with the task's input state. Final picker actions additionally
retain the originating thread for model-transition checks. The original selector,
configuration-preservation and shortcut tests, and final selector delta were read.

Decodex's task model projection is observation-only. Its ordinary per-message
options do not provide this separate task-level selection. Complete the task-level
and exact-live-turn controls together, with explicit scope, current source binding,
native policy checks and outcome readback. Verify that a new task still uses its
previous defaults and that switching tasks does not apply a stale picker action.
Do not add the TUI-specific `s` shortcut or Reserve selection to claim parity.


## MCP status pagination qualification

Upstream `e9633d7a0226eac91c7a791dc4f92cf8f25df2ae` moves owned snapshot
entries into each status response instead of cloning them. Pagination and missing
metadata defaults remain native. The installed-native capabilities test now
compares three one-server pages with the complete list on two fresh process
starts. Healthy, failed-discovery, and failed-initialization states remain distinct.
Evidence: `/tmp/decodex-1254-native.log` (one passing native test) and
`/tmp/decodex-1254-lint.log` (strict runtime lint). This does not test a catalog
mutation between pages or full desktop presentation.


## Apps prepared-call ownership

Upstream `f8bed26f7b180a8fe9df57a6347ae3bceb882ad5` shares equivalent live
Apps catalogs only within matching discovery scopes. Prepared calls retain their
captured definitions. Equivalent peer restoration can preserve them; explicit
refresh on their client invalidates them. Native Codex owns this decision.

Decodex refresh delegates to native reconcile/reload/refetch APIs and does not
replay prepared calls. Two client tests pass in `/tmp/decodex-1256-refresh.log`:
partial reconcile stays visible, and a failed continuation does not replay prior
mutations. Native catalog removal/restoration during approval and explicit refresh
invalidation during approval remain cross-feature acceptance gaps.

Guardian attribution change `b6a5d5bb14d6c3d6a656ca8d2274d6d4259ac57d` preserves
the last received response ID within a turn until a replacement arrives. Decodex
does not supply this ID. Native response-handoff and fresh-turn isolation tests
were reviewed upstream; installed-engine execution of that scenario remains open.


## Command model attribution boundary

Upstream `4d8eca1ff34ff9da717323a7b0a5d0b5ebcf3bb6` captures the invoking
model and effective effort for native command/plugin analytics. The carried
context is omitted from serialized command and approval items. Decodex must not
infer it from the current selected model or a later session-settings observation.

The installed-native local probe `/tmp/decodex-1258-native.py` passes in
`/tmp/decodex-1258-native.log`: a command succeeds, started/completed wire items
omit private model context, and the next request uses the updated model/effort.
This does not validate production analytics delivery or delayed plugin metrics.


## Live model adapter implementation

The native adapter now exposes `LiveModelUpdate` and `update_live_model` for an
explicit model/effort pair bound to a thread and running turn. It shares receipt
parsing with live reviewer publication. It preserves task defaults, permissions,
collaboration mode and service tier by omitting those fields. The runtime must
check the current account catalog and native feature gate before publication.
The retained production bridge now admits this bounded shape. The runtime action
is connected; its desktop selector is connected; full native acceptance remains incomplete.

Five live-settings adapter tests pass, including exact wire scope, applied versus
target-unavailable receipts, explicit native rejection, lost/invalid replies and
no automatic retry. Strict all-target/all-feature codex adapter lint passes.
Evidence: `/tmp/decodex-live-model-adapter-tests.log` and
`/tmp/decodex-live-model-adapter-lint.log`. Full native and desktop acceptance of
the new action remains open. Task-level changes can reuse the existing bounded
`ThreadModelRecoveryUpdate::preserving_service_tier` request and publication
observation; do not create a second native settings authority.

The durable live-settings owner now accepts typed reviewer or model/effort edits
in one reservation sequence. It preserves historical event identities and reads
legacy reviewer payloads. An unresolved model edit blocks a later reviewer edit;
records do not wake work or grant replay permission. Runtime reviewer callers
now use this common owner. Model dispatch and current-turn product controls are connected. Task-level selection remains pending.

Two database tests pass for legacy payloads and reopen/concurrent edit behavior.
The installed-native current-turn reviewer regression still passes (one test).
Strict database/runtime all-target/all-feature lint passes. Logs:
`/tmp/decodex-live-model-store-tests-final2.log`,
`/tmp/decodex-live-model-store-native-reviewer.log`, and
`/tmp/decodex-live-model-store-lint-final.log`.

The `SetLiveModel` action now reaches the shared runtime publication flow. It
requires a model/effort pair in the current native catalog and a positive
thread-scoped `step_model_switching` observation. Account/process/history source
changes reject the edit before reservation or make a submitted outcome unknown.
Protocol version is 2.64. Applied remains a publication receipt, not inference
readback; global defaults and tier are not changed.

The owned-runtime regression passes for applied, lost reply, unsupported effort,
disabled feature and source-change cases, with reopened receipts and no wake or
replay. All 128 protocol unit and six integration tests pass. Strict runtime and
protocol lint passes. Logs: `/tmp/decodex-live-model-runtime-tests.log`,
`/tmp/decodex-live-model-protocol-tests.log`, and
`/tmp/decodex-live-model-runtime-lint.log`. These are synthetic transport checks;
installed-native model switching and desktop acceptance remain open.

The current-turn panel now offers task-bound model choices, effort selection and
an explicit Apply action. It reads choices through the owning task connection,
requires native step switching to be enabled and rechecks the source and receipt
after discovery. Selection is separate from publication. The panel reports the
last requested model/effort and an applied or uncertain receipt, without claiming
that a later inference used it. Navigation and source invalidation clear the panel.

Rendered same-UID socket tests pass for model and reviewer clicks with lost replies
and one command only. The broader live UI filter passes seven tests (three existing
ignored tests). Runtime inspection/publication tests pass; protocol 128+6 and
strict runtime/protocol/GPUI lint pass. Logs:
`/tmp/decodex-live-model-ui-final.log`,
`/tmp/decodex-live-model-inspection-tests.log`,
`/tmp/decodex-live-model-inspection-protocol.log`, and
`/tmp/decodex-live-model-inspection-final-lint.log`.
Installed-native model switching through the runtime action, signed visual
acceptance and task-level selection remain open.


## Installed-native live model qualification

The new `installed_native_live_model_publication_preserves_future_defaults` test
uses the installed Codex binary, the retained bridge and the actual runtime
inspection/reservation/publication flow. A synthetic local Responses endpoint
pauses a dynamic tool after the first inference. Publication does not release
that tool. After its explicit reply, the next inference and captured metadata
change from `gpt-5.6-sol` / `low` to `gpt-5.6-terra` / `high`.

A second native client reads the original persisted task model/effort while the
first client remains paused, with no model request. After the first turn ends,
a cold native process resumes the same thread; the next turn again uses
`gpt-5.6-sol` / `low`. The config file is byte-identical. The durable publication
receipt survives a database reopen. Exactly three model requests occur.

This installed-native regression and strict runtime lint pass:
`/tmp/decodex-live-model-native-crossclient.log` and
`/tmp/decodex-live-model-native-final-lint.log`. The fixture uses synthetic local
responses and no real credentials. It does not qualify signed desktop visuals,
managed-account enrollment, Plan mode transitions or task-level model selection.


## Task-only model reservation foundation

The existing task-model reservation now accepts an optional explicit manual review
identity. Historical automatic attempts omit it and retain their previous key and
payload. Manual requests can change effort without changing the model, including
while a turn is running; automatic recovery remains idle-only. Both use the same
pending-operation check and later native settings publication to record convergence.
A queued reply alone is not publication evidence.

Unchanged manual settings do not create a reservation. Reusing one review identity
with another selection is rejected. A pending manual selection prevents automatic
recovery from overtaking it. The database reopen/publication regression and existing
recovery tests pass (eight tests); strict database/runtime lint passes. Logs:
`/tmp/decodex-task-model-store-tests-final.log` and
`/tmp/decodex-task-model-store-lint.log`.

The task-only runtime action, review query and desktop control are not connected
yet. The runtime must bind the review to current settings, catalog and account,
preserve the native tier and distinguish queued, uncertain and later observed
settings. These database changes do not constitute a delivered task-level control.

Manual task-model convergence compares the requested model and effort. It does
not compare a saved tier: the native update omits tier and preserves the actual
current native value. Installed `ThreadReadResponse` exposes configured model,
provider and effort, but no tier. Automatic recovery can request a tier and still
requires the exact tier match. Manual no-op detection likewise compares only the
fields the request changes. Regression coverage now includes a differing saved
versus published tier and a stale tier on an otherwise unchanged selection.
Eight recovery tests and strict database lint pass; the final added no-op case
also passes (`/tmp/decodex-task-model-tier-tests.log`,
`/tmp/decodex-task-model-tier-noop.log`, `/tmp/decodex-task-model-tier-lint.log`).


## Task-only runtime action

`GetChiefTaskModelSelection` reads native configured model/provider/effort and
compares them with the owned settings journal, then reads the exact account's
catalog. The returned review binds the source, journal record, catalog and latest
operation. It reports the last requested selection, queue/uncertain response and
historical target publication separately.

`SetTaskModel` rechecks that review and the native settings guard, reserves the
shared operation and queues only model/effort. The native tier is omitted. Source
changes or unknown replies never authorize retry. An unresolved prior operation
blocks another edit. The task-level desktop control and explicit cold-restart
reconciliation of unresolved operations remain incomplete.

The runtime regression passes four cases (queued, lost reply, changed account
revision, changed native settings) and checks exact wire scope, persisted response
and no extra request. Protocol 129 unit plus six integration tests and strict
runtime/protocol lint pass. Logs: `/tmp/decodex-task-model-runtime-tests.log`,
`/tmp/decodex-task-model-protocol-tests.log`, and
`/tmp/decodex-task-model-final-lint.log`. Installed-native task-level acceptance
and signed desktop validation remain open.


## Task-only desktop control

The task detail panel now reads task-bound model choices, lets the user select a
model and effort, and sends an explicit `SetTaskModel` action. It does not use
`SetLiveModel` or modify the next-message draft. The result shows configured
settings, the last manual or recovery request, its response, and whether matching
native settings were later observed. Unresolved operations disable further edits.
Global defaults remain outside this control.

Task, thread, turn, runtime-source and disconnect changes discard the review and
draft. A saved change clears the older composer model observation instead of
starting a second unsolicited read. Rendered tests use the same-UID socket and
verify the exact task action, single dispatch after a lost reply, an uncertain
receipt and disabled Apply. Source-change and disconnect checks pass. The first
test exposed a missing disconnect reset, which was fixed.

Two task UI tests and two existing live-setting click regressions pass. Strict
GPUI lint passes. Logs: `/tmp/decodex-task-model-ui-tests-final.log`,
`/tmp/decodex-task-model-ui-live-regression.log`, and
`/tmp/decodex-task-model-ui-final-lint.log`. Task-level installed-native/Plan/cold
qualification, explicit pending-operation recovery and signed visual acceptance
remain open.


## Installed-native task selection qualification

The installed Codex regression now exercises the actual task-selection runtime
in ordinary and Plan modes. It pauses a tool during `sol / low`, queues
`terra / high`, records the real `thread/settings/updated` notification and
reopens the durable receipt. The remaining inference of the current turn still
uses `sol / low`. The process then closes before any new turn can record the new
model. After cold resume, the next inference uses `terra / high`.

A new independent task uses `sol / low`. The config file remains byte-identical,
the native tier remains unchanged, and no tool is released by the settings write.
Plan mode remains present in the settings publication and cold resume response;
the new task does not receive Plan instructions. Each mode produces four model
requests; captured metadata matches each request model and effort.

The native regression (two modes) and strict runtime lint pass:
`/tmp/decodex-task-model-native-cold-final.log` and
`/tmp/decodex-task-model-native-final-lint.log`. It uses isolated local responses
and the existing owned-store fixture. The fixture explicitly journals real native
notifications; it does not replace full signed-app acceptance. Explicit recovery
of unresolved operations after interruption and signed visual QA remain open.


## Interrupted model operation reconciliation

A fresh task-settings read can close an old unconfirmed manual or recovery
operation after its process is confirmed dead. The database checks the new task
owner, current account revision and latest settings observation, then appends a
separate reconciliation record. It neither resends the request nor changes its
original response to applied. Matching historical publication stays separate.
A live old process, stale observation, changed account revision or unowned process
cannot release the reservation. The original request identity remains spent.

The UI keeps the old queued/uncertain response and explains that current settings
were reviewed after restart. It enables a new explicit selection only after
reconciliation. This also prevents an interrupted automatic recovery operation
from permanently blocking manual selection.

Nine database tests pass, including matching and differing current settings for
both manual and automatic requests. The runtime publication regression, three
rendered task-selector tests, 129 protocol unit and six integration tests pass.
Strict database/runtime/protocol/GPUI lint passes. Logs:
`/tmp/decodex-task-model-reconcile-tests-final.log`,
`/tmp/decodex-task-model-reconcile-runtime.log`,
`/tmp/decodex-task-model-reconcile-ui.log`,
`/tmp/decodex-task-model-reconcile-protocol.log`, and
`/tmp/decodex-task-model-reconcile-complete-lint.log`.
The database death-evidence fixtures do not constitute a signed-app crash test;
full actor and signed desktop acceptance remain separate.

## Permission selection during a running turn

Upstream `2b59d92dbdd69fc600ed95250867d305942912e1` routes builtin profiles through
native settings publication and permits them during a turn. Named profiles still
require idle. Decodex now exposes per-profile `can_select` separately from native
policy `allowed`. The database permits only builtin IDs while running, rejects the
dispatch transition, and preserves the active turn. Native publication confirms
the target; an ACK remains queued and an uncertain response is not replayed.

A separate configured-permission observation reads the latest published task facts
under a current settings/history guard. It does not claim the active inference's
sandbox and does not relax the existing idle-only observation used by other callers.
Malformed publications, history reverts and source changes still invalidate reads.
The coordinator can now confirm a running builtin selection from its publication.

The installed-native fixture pauses a tool, rejects a named selection, changes a
builtin through the runtime and retained bridge, verifies the publication and
reopened receipt, then explicitly releases the tool. No extra inference or config
file write occurs. Evidence: `/tmp/decodex-1397-running-native-final.log`.
Adapter, database, runtime and rendered permission tests also pass. A model operation
reconciled after confirmed process death no longer blocks a later permission edit;
its original uncertain delivery record is retained.
Signed desktop and combined full-service restart acceptance remain separate.

## Native MCP root interaction (1426)

Upstream `40584fad87aa2cd63e03db4784ccfd5b50a59bed` limits human MCP input
to native root sessions. Do not reinterpret an empty schema as consent when
`codex_approval_kind=browser_auth` or `codex_requires_user_input=true` is present.
Native code rejects child requests before review/prompt registration and returns
parent-handoff guidance; automatic permission and review decisions retain their
existing policy. Chief's ancestry lookup still applies to genuine native requests
(such as child shell approvals), not model text. No synthetic prompt or retry is
introduced for this new refusal.

Installed native test `native_child_mcp_input_returns_handoff_without_local_prompt`
passes both markers under Full Access through the real Chief coordinator. It
checks the native MCP error, no pending local request, one tool invocation and
the exact call's subsequent model input. Evidence:
`/tmp/decodex-1426-native-child-mcp-final.log`, full runtime lint:
`/tmp/decodex-1426-lint.log`. The initial test incorrectly classified all prompt
text containing "root thread" as a tool response; it now uses the exact call ID.
No production behavior was changed to make the fixture pass. Root browser UI,
automatic-review combinations and signed desktop remain separate acceptance work.

## Captured child settings (1433)

Upstream `800d183e` derives a new child's model, effective reasoning effort and
summary from the invoking step. An active-turn update must not leave the child
with initial turn settings. Both native spawn versions preserve role/explicit
model precedence and validate effort against the captured model. Final changes
do not undo that ownership. Decodex must keep native spawning and live-settings
publication together instead of injecting its own stale child defaults.

The installed-native test pauses a sol/low parent, writes terra/high through
`chief_live_settings::write`, releases the tool and creates a native V2 child.
Exactly one child request uses terra/high and completes under the parent's native
identity. The test uses a local deterministic Responses server and no live model
service. It passes in `/tmp/decodex-1433-native-child-final.log`; full runtime
Clippy passes in `/tmp/decodex-1433-runtime-lint-final.log`. V1, effort-only
validation, summary inheritance and signed cross-client UI combinations remain
separate qualification cases. Initial fixture classification assumed a user
message; native V2 uses `agent_message`, so the final assertion uses exact parent
and subagent request metadata.

## Model-owned native tool descriptions

Upstream `c5d079470eeaf9502080faa0697aade481242081` resolves the V2 spawn tool's
static description from `model_messages.tools.multi_agent.spawn_agent.description`.
Missing and null values preserve the built-in text; an empty string removes its
static text. Runtime model guidance, local usage hints and parameters remain.
The active step's model selects the description. The cutoff extends this native
owner to all six V2 tools and optional parameter schemas. The wrapper preserves
encrypted parameter annotations and falls back for invalid schemas.

The installed `0.155.0-alpha.16.3` fails the new catalog-description qualification.
The synthetic catalog is loaded (its model labels appear in generated guidance),
but the outbound spawn tool still has its built-in description. Evidence:
`/tmp/decodex-1445-native-catalog-diagnostic.log`. This is an unresolved native
compatibility result, not a passing adaptation. No local prompt replacement was
added. The original child-model test remains separate from the strict ignored
`installed_native_spawn_description_follows_updated_step_model` qualification.
Re-run that qualification against the intended supported native runtime before
claiming catalog descriptions work. Final schema overrides, empty/null cases and
combined signed UI acceptance are not qualified by the existing model-switch test.

## Guardian sender evidence

Upstream `e269f2164cbb9f499e4f22301c393500e2a831f3` captures up to three local
sender user messages when native admission accepts a `send_message_to_thread`
delivery from `codex_app` or `codex_tui`. It requires thread-owned Guardian context,
a standalone function output and host-resolved sender provenance. The bounded
snapshot is reviewer-only historical evidence, not transferred permission.
Missing, incomplete or over-budget messages produce explicit notices. The latest
delivery survives replay and compaction, follows rollback order and is stripped
from forked agent history. Both reviewer modes require this evidence under budget
pressure instead of silently omitting it.

Read the upstream admission, retention, composition and replay/fork tests. Final
source retains this behavior after the LocalAgentControl rename. Decodex has no
`codex_app` or `codex_tui` sender-delivery producer; its ordinary user-message and
native collaboration paths must not forge that namespace or harness metadata.
This specific delivery feature has no current local producer to adapt. Native
Guardian cross-thread, restart and signed UI acceptance remain separate gaps;
the upstream tests were inspected, not executed here.

## Guardian cached-score publication

Upstream `fcf05456bb27e6c3d5677550f54011db6a2a0817` owns score, authorization,
coverage and observation state under one mutex. Approval reads consistent
snapshots, including a second read after asynchronous authorization collection.
Delayed or equal-timestamp successful results cannot advance coverage; failures
win timestamp ties. Oversized evidence remains attached to its active call until
completion, even after another score succeeds. Final score owners are unchanged.

Reviewed the production publication and approval paths, score ordering tests and
the gated integration sequence that distinguishes missing, stale and fresh
scores. Most existing fixture conversions were sampled, not exhaustively reread.
Decodex's `chief_guardian.rs` reads saved native review observations; it does not
maintain a SecurityRiskScore cache or grant actions from one. Native concurrent
scoring, authorization-change races and signed UI acceptance remain unverified.

## Guardian reusable history prefix

Upstream `d7f8e48d7d9211e169b5b4b438798e21caa923a4` orders root/sender evidence,
retained instructions, trusted user answers, transcript and permissions before
changing previous-review, tool and skill attestations. Current action evidence
remains last. History stays user-role evidence. The public composer regression
varies attestations/actions while preserving the history prefix for both legacy
and retained-context histories; the app-server fixture checks separate transcript
and action messages. The cutoff composer is unchanged.

Decodex does not construct Guardian requests or retain a second prompt-prefix
cache. Native repeated-review cache-prefix behavior remains an acceptance gap;
the earlier cold-image and trusted-answer tests do not prove this new property.
