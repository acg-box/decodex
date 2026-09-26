# Codex integration review: current status

The fixed upstream cutoff is `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
The manual update is incomplete. Use [Upstream adoption review](work/upstream-adoption-review.md)
for core and optional classification, and [Inherited change reconciliation](work/upstream-inherited-audit.md)
for source preservation. Source comparison, native qualification, merged delivery
and signed desktop acceptance are separate results.

The original review record below is restored byte for byte. Its 1460/1569 cursor,
next-commit instructions, version numbers, unmerged labels and intermediate missing
features are historical. Do not restart the scan from that cursor or implement an
optional proposal solely because it appears in the historical record. The 1569
commits describe the scan range, not the number of adopted product features.

Current prompt editing, MCP App UI, task permissions, live/task model controls and
native policy routing have later implementation and reconciliation records. They
must be assessed through their current feature notes. Analytics, collaboration-mode
selection, memory readiness controls and other optional proposals are not delivered
merely because their native schemas were discovered. Native/TUI/Windows/Linux
internals without a Decodex consumer do not require local replacement code.

The old spawn-description failure has later positive qualification, while the old
child MCP handoff success is contradicted by the current installed binary. Explicit
Flex cold recovery also remains unqualified. Read [native qualification boundaries](work/source-preservation-native-limits.md)
for exact evidence. Historical test success is not current signed desktop acceptance.
The signed 0658 artifact predates subsequent fixes and is not the final artifact.

Automations remain paused, including after manual completion, until the user
explicitly requests resumption. This instruction supersedes historical scheduling
or completion instructions in the restored record.

## Original historical record

# Codex integration review

## Current manual review: 2026-09-23

The fixed upstream cutoff is `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Consecutive source review covers 1460 of 1,569 commits from the historical lower
boundary `a397079287e6638b39dda329835350d93222681f`. The next commit is
`0a5b9991698e8e3c126da6101aa9e4da421f7ddd`. The lower boundary is not a certified
audit of earlier changes. The manual catch-up is incomplete; the maintainer stays paused.

### Reviews 1458–1460

- `8b78600d` preserves explicit Windows MXC selection through configuration,
  environment attachment, command execution, patch writes, stdin approval and
  metadata. Read primary consumers, configuration/selection/TUI tests and Wine
  routing coverage. Repeated constructor fixture changes were sampled. Legacy
  implementation requirements still do not constrain MXC. At cutoff, private
  desktop settings are removed and MXC managed networking requires local binding.
  Decodex has no Windows sandbox configuration consumer; do not add one to its
  macOS path. Windows execution was not tested here.
- `3ed49879` changes only upstream R2 release upload settings: classic transfers,
  one concurrent part per object and standard retries with six attempts. The
  cutoff workflow is unchanged. No matching release upload owner was found in
  Decodex scripts; no product or app-server adaptation is required.
- `d7f8e48d` places changing Guardian review/tool/skill attestations after history
  and permissions, before the current action. Read the full patch and cache-prefix
  test, including retained and legacy history. The final composer is unchanged.
  Decodex consumes native review events instead of composing this prompt. Native
  repeated-review prefix behavior is not yet qualified; this is source evidence.

### Reviews 1456–1457

- `608825d5` adds visible single-grapheme accents, symbols and named delimiters.
  The ahead-of-cursor GPUI adaptation already contains these rules. Reread the
  production delta and upstream tests; final renderer is unchanged. Four current
  math behavior/rendered tests pass. Signed desktop acceptance remains open.
- `3d3ae496` adds borrowed filesystem access with captured permissions and opaque
  weak-identity cache keys. Read the complete accessor and local/remote regression
  tests. Open streams outlive accessors; the unrestricted constructor is still
  available. Decodex uses the native app-server owner, not these internal crates.
  No public protocol adaptation is introduced; later consumer migrations remain
  part of the unread review. See `work/filesystem-policy-cwd.md`.

### Reviews 1450–1455

- `c11fdc94`, `32b54cff` and `2833985d` improve Windows provisioning error chains,
  empty registry cleanup, service-first repair and runtime-child read/execute ACLs.
  Read all changed production and regression tests. Repairs preserve denials and
  skip reparse points. Final source adds pending-cleanup state to registration;
  the reviewed provisioning and ACL behavior remains. No corresponding macOS
  Decodex owner requires migration. Windows execution was not tested here.
- `fcf05456` publishes Guardian score, authorization and covered call index under
  one lock. Read publication, observation, approval snapshots, timestamp and
  delayed-score tests. Final score owners are unchanged. Decodex projects native
  review events and has no separate risk-score cache. Native concurrent scoring
  acceptance remains open; source inspection does not prove the installed race fix.
- `96aca987` maps prepared image IDs back to original user-input positions before
  emitting display history. Read mapping, preparation and live/persisted tests;
  final mapping is unchanged. Existing mixed file/inline native history and cold
  continuation qualification passes again on alpha.16.3. It does not cover an
  injected uploading store or file-ID byte resolution. See stored-image notes.
- `16f49ccd` removes one default V2 delegation-guidance sentence; no parameters or
  execution behavior change. Final description is unchanged. Decodex does not
  duplicate this text. Installed catalog-description qualification remains failed
  as recorded under 1445; this source change does not alter task delegation policy.

### Review 1449

`7abf2a3b` preserves explicit Flex without fast-mode or catalog support and omits
tiers from native Bedrock requests. Reviewed production and startup/settings,
request, review and TUI tests. Decodex already preserves and forwards the setting.
The new native qualification exposes an installed-runtime gap: alpha.16.3 accepts
the settings update but omits Flex from its outbound request with fast mode off.
Keep that failure open; details are in `work/service-tier-flex.md`.

### Reviews 1446–1448

- `1bd1bfa7` identifies the opened daemon socket mount through fdinfo/statx and
  checks ancestry, exposed aliases and nested mounts. WSL masks persist through
  proc preflight. Read production, layout tests and disposable namespace fixture.
  Final source only parses path roots on the socket filesystem, allowing unrelated
  nsfs roots without dropping their destination checks. This Linux-native owner
  has no macOS Decodex implementation to migrate; Linux execution was not tested.
- `b0659c53` records actual CLI/TUI daemon connection and update outcomes with
  consent, bounded setting-presence tags and one handoff observation. Read consent,
  launch and handoff tests. Final daemon feature overrides do not change telemetry
  ownership. Decodex's supervised stdio launch does not enter these CLI daemon
  paths; no duplicate telemetry exporter or daemon startup is added.
- `e269f216` retains reviewer-only sender evidence for accepted native desktop/TUI
  delegation deliveries. Read admission, budget, replay, rollback and fork behavior.
  Decodex has no producer for that specific host delivery protocol. Preserve native
  ownership and do not invent provenance; see `work/chief-upstream-integration.md`.

### Review 1445

`c5d07947` selects model-owned V2 spawn descriptions from the active step's
catalog. Reviewed production, sparse/empty serialization, generated guidance,
outbound schema and mid-turn change tests. At the cutoff, the native wrapper also
supports other V2 descriptions and parameter schemas, preserving encrypted fields.
Decodex already leaves native tool construction to app-server. However, the
installed alpha.16.3 fails the new catalog-description qualification despite
loading the synthetic catalog. This remains an explicit compatibility gap in
`work/chief-upstream-integration.md`; source review is not feature acceptance.

### Review 1444

`a4ee536f` separates read and write sandbox dispatch. Full-disk reads can bypass
the sandbox helper without granting mutation rights. Reviewed local and remote
dispatch, skill and discovery consumers, permission conventions, direct-read
regressions, old discovery capability handling and Windows write tests. Final
source retains these decisions. The installed executor passes ten isolated checks,
including four denied mutation APIs with unchanged files. See
`work/filesystem-policy-cwd.md` for evidence and limits. No Decodex permission
evaluator replaces the native owner; Chief and signed desktop acceptance remain.

### Review 1443

`70e8fe1b` adds opt-in daemon startup to interactive CLI launches. Read startup,
connection failure handling, exclusions, experimental persistence and CLI/TUI
regression tests. The toggle defaults off and persists explicit false. Eligible
automatic startup requires a connection; excluded launches retain embedded mode.
Final source adds worktree preparation, four shared-service feature compatibility
checks, structured code-mode fallback checks and launch telemetry. Compatibility
failure selects embedded mode; startup failure retains manual fallback guidance.

Decodex launches an account-bound `app-server --stdio` through its existing
supervisor, not the interactive CLI startup owner. No daemon installation,
automatic startup or TUI flag is added to that command. Native feature discovery
already accepts an optional thread scope. No local behavior change is required
for this CLI-only option. Upstream tests were read, not executed; this conclusion
does not certify shared-daemon use by Decodex.

### Review 1442

`4b0f19d6` makes web and image activity compact in the TUI while retaining full
details. Reviewed action rendering, full image paths, owning MCP image results,
live/replay tests and transcript export. Final search and image owners are unchanged;
later MCP changes use the shared three-row preview reviewed under 1432. Later
transcript changes concern asynchronous question replies.

Decodex now distinguishes search, open-page and find-in-page activity. Its existing
source-bound detail reader exposes all search queries, full URLs, patterns and
image paths through the existing paged disclosure. Credential filtering remains
in that reader. No image bytes or path-derived media permissions are inferred.
MCP attachments already belong to their native item. TUI row budgets and keyboard
shortcuts do not define GPUI presentation.

Validation: nine runtime detail tests, one action-label matrix and one rendered
socket pagination test covering four item kinds pass. Full runtime and GPUI
Clippy contracts pass. Signed desktop acceptance and combined native replay remain
open; these checks do not establish full catch-up or merged delivery.

### Review 1441

`841b5490` separates selected filesystem policy cwd from helper launch cwd.
Strict internal contexts retain cwd and roots; additive wire policy context keeps
legacy clients compatible. Dynamic legacy permissions without cwd fail closed;
static legacy requests use the executor's own cwd. Foreign permission paths are
validated at executor ingress. Helpers can launch from the filesystem root after
the selected directory disappears without moving permission anchors. Read core
propagation, wire conversions, legacy read/open/discovery/process matrices,
removed-directory patch test and Windows relative-denial tests. Mechanical
fixture constructor updates were not exhaustively reread. Final owners retain
policy context; later read/write-specific dispatch still validates foreign paths.

Added an isolated installed-executor probe. On CLI `0.155.0-alpha.16.3`, five
checks pass: modern and legacy allowed/denied reads after selected cwd removal,
and rejection of a missing cwd for dynamic permissions. Native executor version
metadata is `0.0.0`; CLI version is recorded separately. Missing-cwd error code is
`-32600` here versus `-32602` in fixed source; both reject the request. Decodex does
not speak this internal executor protocol. Full Chief/apply-patch, remote Windows
and signed desktop acceptance are not proved by this direct-executor probe.

### Review 1440

`108e6a6d` removes persistent Sites migration exclusions and catalog-time migration
waits. The runtime loader instead suppresses bundled Sites when the active remote
catalog has a loadable cached remote Sites entry, including a disabled entry.
Without that bundle the bundled fallback remains. Catalog listing and install
resolution no longer hide the old identity using migration state. Read the loader,
manager/caller changes, removed catalog assertions and the three-case agent-turn
regression. Mechanical install-signature substitutions were inspected as such.
Final loader guard is unchanged; later manager changes concern metadata refresh
and onboarding skill paths.

Decodex reads `plugin/installed` for the exact task cwd and preserves native IDs,
enabled flags and marketplace errors. It has no Sites-specific exclusion state or
same-name deduplication. No local migration or client-side blacklist is required.
This source review does not newly qualify remote plugin service/cache behavior in
the installed binary. Sites plugin compatibility is distinct from the retired
Decodex website.

### Review 1437–1439

`f3da3861` adds a brief Astra starfield to the TUI composer. Scoped review covered
new/resumed/forked task identity, model-picker action routing, queued automatic
model updates, normal/image submission, replay dismissal, display eligibility
and final deltas. Read fresh-task and picker-race tests; did not exhaustively
review raster math or decorative snapshots. The action wrapper still dispatches
the existing model operation and gates only animation on the original task and
confirmed effective model. No protocol/runtime feature is introduced. GPUI uses
its own presentation and source-bound model selection; do not add terminal
starfield rendering as a compatibility requirement. Existing signed model and
blank-task acceptance gaps remain independent.

`16f59db9` pauses terminal events in the fresh-task regression test. No production
change or local test migration. `77c1feb0` boxes the initialized app-server request
future to reduce stack temporaries. Final dispatch retains admission checks,
serialization and error delivery; only account-processor sharing changes later.
Native Codex owns this internal executor. No client request/response or local
async ownership change is needed.

### Review 1435–1436

`8452164c` marks a finished, losslessly observed Code Mode cell complete even when
it invoked no tools. Complete metadata includes `executed_tool_calls: []`; absence
still means unverified. Read retry/history revalidation, late-call, wait freshness,
error/termination and feature-gating tests. Final recorder/protocol owners are
unchanged. Decodex does not interpret this internal metadata as tool success or UI
loading state. Extended installed-native compaction/restart qualification passes
for tool execution, discovery-only output and a thrown error. Both next model and
compaction requests retain exact completeness metadata. Full runtime Clippy passes.

`787823cf` adds TUI `--no-daemon`, bypassing daemon discovery even when running,
and rejects agents/queue/remote combinations that need another owner. Read launch,
resume/fork propagation, negative CLI and PTY discovery tests. Final startup policy
adds compatible-feature checks, explicit startup failure handling and telemetry;
`--no-daemon` still excludes discovery/startup. Decodex launches its attested
`app-server --stdio` directly, not the interactive TUI entrypoint. Do not append a
TUI option to this command or start a shared daemon to mirror terminal behavior.

### Review 1433–1434

`800d183e` builds child model, effective effort and summary from captured step
settings after active-turn updates. Both spawn versions validate an effort-only
override against that captured model. Runtime permissions/cwd still come from
the turn. Read production, unit and four upstream real-turn cases, plus final
deltas (agent type moves, description overrides and environment permissions).
Installed-native qualification through Decodex live-settings write passes: a
paused sol/low parent switches to terra/high, then its new V2 child makes exactly
one request with terra/high and completes under the correct parent identity.
Full runtime Clippy passes. V1, explicit effort overrides, summary and signed
cross-client combinations are not covered by this added native case.

`1e9564fb` keeps the TUI startup composer responsive during config/trust/hooks,
thread creation and attachment. It retains a failed draft for retry, does not
send a first turn and skips descendant scans for new sessions. Final attachment
marks only immediate creation as fresh. Read production/test changes and final
delta. Decodex still lacks native blank-session/worktree creation; existing draft
retention is not equivalent. Added the responsive draft/retry requirements to
[Command Center](work/command-center-upstream.md); implementation remains open.

### Review 1432

`6749535c` bounds Code Mode previews across result blocks after wrapping and keeps
complete transcript output. Its initial head/tail preview is superseded at the
cutoff by the shared three-row `ToolOutputPreview`, with hidden-line counts,
UTF-8/combining-character work bounds and full expanded text. Read initial tests,
final MCP consumer and final preview implementation/tests.

Decodex MCP and dynamic calls already use compact activity rows and source-bound
paged details. Found and fixed a separate gap: standalone `functionCallOutput`
was shortened to an 8 KiB preview without a detail entrypoint. It now uses the
same full native detail reader, privacy filter, cursor and GPUI disclosure. Text
blocks retain order and trailing diagnostics; media remains separate. Runtime
five-test selection and rendered two-kind pagination pass, as do full runtime
and GPUI Clippy. Signed desktop/native standalone-output acceptance remains open.

### Review 1431

`b974893c` charges file images in native context and compaction budgets, preserves
file references in Guardian composition, and accounts for reference-byte/count
limits and reviewer deduplication. Read the full production/test patch and final
owner deltas. Original-detail file images reserve the maximum patch count because
reference IDs do not contain dimensions. No local token-budget replacement.

Installed-native cold-resume qualification passes: the primary model retains two
file references in order; synchronous Guardian retains the user restriction and
uses its native text-only profile. Initial test expected image admission, but
source confirms synchronous collection passes `images: None`; asynchronous
Guardian has a different profile. Full runtime Clippy passes. Native image-budget
thresholds and asynchronous admission remain unqualified. See
[Stored images](work/stored-image-references.md) for exact boundaries.

### Review 1427–1430

| Upstream commit | Local applicability and evidence |
| --- | --- |
| `e22e6523`: completion timestamp labels | Removes the TUI `done` prefix. Typed separator normalization now preserves timestamp-like answer text; cumulative duration and interrupted/failed status tests remain. Final separator owner unchanged. GPUI renders structured turn status and duration through `chief_timeline_render` and `reply_metrics`, with no parsed TUI label. No protocol or local label migration is required. |
| `172f8a29`: inline code and path colors | TUI uses syntax-theme markup scopes with a cyan fallback. Read dark/light/ANSI/fallback and test-thread theme isolation changes. Final markdown and streaming owners unchanged; later highlight changes add theme resolution and diff background handling. GPUI uses its own text/background/link palette, not terminal syntax themes. Do not import the TUI theme registry for this presentation change. |
| `36b84c81`: skill budget warnings | Native skills stop warning for shortened descriptions but still warn when skills are omitted. Catalog accounting and nonempty shortened descriptions remain; app-server test explicitly forces omission with a 1,000-token budget. Final rendering owner unchanged. Decodex projects native warning events and has no duplicate skill-budget warning producer. No client-side message suppression is needed. Installed-native warning behavior was not newly tested in this batch. |
| `ce03f22a`: configurable voice toggle | TUI F8 starts/stops through the existing voice owner, only on press outside popups. Existing bindings/chord prefixes win; printable input and reserved shortcuts cannot be stolen. Read remapping/unbinding, draft preservation, side-conversation guard and conflict tests. Final keymap/interaction owners unchanged. GPUI has native start/stop controls and a separate desktop keymap; it does not consume `tui.keymap`. Desktop keyboard access remains to verify with signed voice acceptance; no F8 equivalence is claimed. |

### Review 1426

`40584fad` prevents native non-root MCP requests from opening user prompts.
Browser-auth and explicit user-input metadata take precedence even with an empty
schema and Full Access. Automatic permission/reviewer decisions remain available;
requests with no automatic decision return parent-handoff guidance. Reused MCP
connections receive current interaction authority. Authentication diagnostics are
preserved subject to normal output limits. Read the production path, policy and
reuse tests, real-turn root/subagent tests and final delta. Final prompt guards are
unchanged; later session changes concern environment/auth refresh and step inputs.

Decodex resolves ownership only for actual native server requests. It does not
create prompts from handoff text or retry the refused action. New installed-native
Chief test passes for browser-auth and explicit-user-input markers under Full
Access: no local pending prompt, one MCP call, exact child gets handoff output.
Full runtime Clippy passes. Native automatic-review/root-browser UI combinations
and signed desktop acceptance remain separate from this two-case test.

### Review 1425

`5e636ea7` routes resized message/tool images through the injected attachment store,
preserves inline data on upload failure and avoids uploads during history replay.
Guardian image comparison reuses resizing without upload. Final preparation/store
owners are unchanged; ordinary app-server still injects the inline store. No new
public upload/resolution RPC is available. Extended installed-native qualification
passes for cold continuation with exact file/inline/file ordering and no duplicate
images in both notification-media modes. See [Stored images](work/stored-image-references.md)
for the distinction from remote upload, resizing and signed display acceptance.

### Review 1421–1424

| Upstream commit | Local applicability and evidence |
| --- | --- |
| `f915e0de`: streamed Mermaid rendering | Read renderer, closing-fence logic, theme roles, nested-block holdback, resize/raw-mode progress and tests. Final renderer/controller unchanged. Existing Decodex native diagram renderer already preserves source, supports nested/incomplete fences and scrolls without wrapping; corrected comma-delimited fence info recognition. All 18 Mermaid tests and full GPUI Clippy pass. Signed font/long-diagram acceptance remains open. |
| `8ace915a`: realtime analytics attribution | Internal reducer tags turn/App/MCP/skill events with the active voice session, queues a handoff after voice closes and avoids tagging the next text turn after steering. Only the handoff marker enters telemetry, not speech. Final attribution logic unchanged; tests moved to reducer-ordering suite. No new app-server field. Local voice receipt identity is a separate durable record; do not equate it with native analytics attribution or duplicate native telemetry. |
| `e412b93d`: Guardian test trimming | Removed standalone tests and strengthened exact transcript separators/action header/parent ID assertions before snapshot normalization. No production change. Decodex retains its native reviewer and receipt tests; removal upstream does not remove a local requirement. |
| `08663cc9`: shared Guardian test boundaries | Approval-policy/reviewer matrix moves to the shared routing owner; parent config isolation and manual-approval developer-message preservation remain covered. Production routing file only adds a test module. Final routing/shared transcript test owners unchanged. No local routing replacement or new protocol field. |

### Review 1417–1420

| Upstream commit | Local applicability and evidence |
| --- | --- |
| `51c30ad8`: expired Windows sandbox passwords | Full setup repairs expired offline or online passwords and reloads protected credentials; refresh cannot rotate them. Tests cover either account, failed repair and preservation of enterprise flags. Final code also removes stale credential files and prefers the installed service, with fallback only when unavailable. Current Decodex macOS does not provision Windows sandbox accounts. |
| `20f4d12f`: Code Mode compaction metadata | Inspected pending/retained metadata, shortened retry preservation, separate Code Mode budget and direct-history binding checks. Final metadata owner and remote-v2 call site unchanged; later local compaction separately stages post-turn output until success. Installed-native MCP/helper-to-compaction and cold-checkpoint continuation test passes. See [Code Mode](work/code-mode.md) for exact remaining cases. |
| `821ad43f`: rollout compression trigger metrics | Startup and RPC entrypoints now tag run/file/scan/cleanup counts, timings and sizes; materialization errors have no trigger. Final compression owner unchanged. Best-effort RPC acknowledgement is still not completion. Decodex uses native thread history and does not implement rollout compression or consume these counters. |
| `c56dda71`: WebSocket continuation metrics | Inspected first reset-reason retention, full versus incremental input, warmup/generation, resume/fork/account-switch tests and metric semantics. Final continuation owner unchanged; later client changes concern effort updates, Bedrock tier and auth retry classification. Native Codex owns model WebSockets; Decodex local service WebSocket is a different transport. The counter measures send attempts, not disconnect rate or cache reuse; no new local app-server DTO is required. |

### Review 1416

`b97abdbe` adds native MCP `openai/readOnly` metadata to discovery and invocation,
preserves arguments/pagination/other metadata, overrides a caller-supplied false,
and isolates connections and tool caches. Inspected modern/legacy request tests,
recovery tests, shared Apps-cache and connection-reuse tests, and the final owners.
The final ordinary `Config::mcp_config` still sets the policy false. The installed
alpha.16 public schema has no `requiresReadOnlyMcpTools` field. The added scenario
source contains only a comment, so its checked-in snapshot is not evidence of an
active end-to-end test. Decodex delegates native MCP transport and has no competing
tool cache. Do not advertise MCP read-only enforcement from the filesystem sandbox
profile or add an unsupported app-server field. This is native infrastructure at
the fixed cutoff, not a newly exposed Decodex setting.

### Review 1412–1415

| Upstream commit | Local applicability and evidence |
| --- | --- |
| `4fa7e822`: preserve reviewer config error causes | Final TUI handler retains `format_config_error`; the real malformed-config regression is unchanged. Local account and connector config reads/writes now retain bounded actionable RPC causes in source-owned, non-waking task diagnostics. Private error data is excluded and credential-like messages are hidden. Unknown writes stay unknown and are never retried. Installed-native malformed-config test and host reopen/diagnostic tests pass; signed display acceptance remains open. |
| `4cf84b76`: Windows sandbox DACL tests | Adds owner WRITE_DAC permission and sandbox-group denial assertions, and releases the SID allocation. No production change; final test file is unchanged. Decodex's current macOS desktop does not implement Windows sandbox ACL provisioning. |
| `515530d9`: screen-reader animation default | Inspected one-time 450 ms detection, both marker values, explicit/user-layer/CLI precedence, concurrent edits, failed persistence, platform probes and rendering/startup tests. Final detection and local-settings owners are unchanged. Decodex uses native desktop preferences, not TUI configuration: VoiceOver now joins Reduce Motion in the existing animation owner. Existing transition tests and native-compiled lint pass; signed VoiceOver acceptance remains open. |
| `78e7825a`: Guardian test request identity | Tests force HTTP, inspect the last yielded request and wait for the Guardian metadata marker before cancellation. Final helper retains that marker; later test changes include step-input/environment refactors. Existing Decodex native Guardian fixtures explicitly disable WebSockets and classify Guardian requests by the same metadata. No production protocol change or new local retry behavior. Broader Guardian acceptance remains tracked separately. |

### Review 1405–1411

| Upstream commit | Local applicability and evidence |
| --- | --- |
| `a8c36ca6`: central model-message rendering | Inspected message-family resolution, permission rendering, Guardian composition and call sites, model serialization, and behavior tests. Missing versus empty overrides, configured precedence, literal templates and content attribution remain distinct. Final prompt-owner changes add multi-agent tool catalog accessors. Local Chief supplies coordination instructions, not a replacement model-message renderer; Guardian presentation distinguishes timeout from denial. No local wire migration follows from the internal crate move. Combined native acceptance remains open. |
| `47c27cbf`: domain question-mark patterns | Documentation and tests specify that `?` matches one character, including a dot, in allow and deny entries. The final matcher is unchanged. Decodex has no competing proxy hostname matcher. |
| `2b2b0fa8`: browser cleanup on interrupt | Native bundled-hook authorization adds Interrupt for the registered browser connector, with empty arguments and existing policy checks. Final authorization code is unchanged. Local ordinary interrupt sends the exact native turn interrupt and retains the process; no local browser cleanup call is required. Signed interrupt and shutdown acceptance remain open. |
| `105fe876`: Noise relay handshake cooldown | After eight failures, native admission pauses for ten seconds while existing streams and admitted validations continue. Tests cover encrypted round trips, recovery, duplicate handshakes and early data. Final owner is unchanged. Decodex uses the local stdio process bridge and does not implement this remote relay. |
| `73bf1812`: forced macOS managed preferences | Native loading checks forced status before and after reading and type-checks the value. Tests cover ordinary defaults, disappearing force, missing values, strings and redacted invalid-type errors. The final owner is unchanged. Decodex has no independent managed-preference loader; native MDM acceptance remains open. |
| `29e6bc81`: orchestrator skill cache reuse | Native caches use a weak server connection identity and invalidation generation. Plugin changes and explicit refresh invalidate resources; ordinary publication can reuse them. Final code also separates reprojection from refresh and adds auth-scoped cache identities. Decodex does not own this resource cache; existing native plugin reload remains authoritative. |
| `a6d4741d`: per-App tool exposure | Confirmed a local settings gap. Installed-native config write, inheritance, cross-client conflict and restart checks pass, and the local host, durable reservation, service and settings control are implemented. Full signed/native tool-exposure acceptance remains open. See [App tool exposure](work/app-tool-exposure.md). |

### Review 1400–1404

| Upstream commit | Local applicability and evidence |
| --- | --- |
| `bee042d1`: managed residency at provider construction | Native provider construction now overrides configured residency headers for model and realtime requests. The final enforcement owner and original provider/realtime tests were inspected. Direct quota activation bypasses this owner and still needs adaptation; see [workspace routing](work/workspace-backend-routing.md). No new native residency acceptance is claimed. |
| `a2f62e88`: shared permission shortcut selection | The final shortcut and selection owners retain the notification-based confirmation path. Local permission selection already records queued, unknown and observed states separately; the GPUI panel does not treat an accepted command as an applied profile. The prior running-profile native test covers this distinction. There is no separate local shortcut that optimistically changes permissions. |
| `fc2ea82e`: disable executor skills per environment | This is a native extension-provider API, keyed by environment and exact skill-document URI. Its direct and cached discovery tests retain other environments and omit disabled skills from model context. The final owner retains this behavior. Decodex does not construct executor skill providers or inject a competing skill catalog; no local API migration is required. |
| `6d75b52`: hosted Apps MCP protocol override | Native registration keeps hosted provenance when selecting a protocol version. Original tests distinguish a hosted contribution from a custom server with the same name and preserve native user verification. Final contribution and registration code retain that distinction. Decodex does not choose MCP transport versions; widget presentation and signed verification acceptance remain separate open work. |
| `0d083092`: experimental rollout compression | The RPC schedules a best-effort local-store pass and returns an empty acknowledgment. Final code adds the RPC telemetry trigger without changing completion semantics. Original tests cover experimental gating, non-local refusal and lossless readback. Decodex reads native history through RPC and has no rollout-file reader or manual compression surface to migrate. Do not add automatic calls or report completed compression from this acknowledgment. |

Native summary recovery after full history failure is implemented. Native restart,
source-change and rendered copy tests pass; signed desktop acceptance remains open.
See [history recovery](work/history-summary-recovery.md).

MCP App widget presentation remains unimplemented; see [MCP App UI](work/mcp-app-ui.md).
Editing an earlier prompt also remains unimplemented; existing revert invalidation
does not supply the full action. See [prompt editing](work/prompt-editing.md).

Mermaid code fences now use the upstream bounded native text renderer. Parser and
GPUI scroll/copy tests pass; signed desktop visual acceptance remains open. See
[Mermaid rendering](work/mermaid-rendering.md).

Account analytics remains unimplemented. Typed upstream report contracts and final
normalization differences are recorded in [account analytics](work/account-analytics.md).
Account-bound report reads and the dashboard still need integration. Model workspace
routing is not a prerequisite for Analytics; direct activation routing remains open.

Desktop math now uses the final upstream bounded parser, including display
fractions and accents. Formula scrolling and source-copy interaction tests pass;
signed desktop acceptance remains open. See [math rendering](work/math-rendering.md).

Interrupted answer and plan text now survives terminal handling, database reopen
and the next turn as display-only records. Native and rendered fixture tests pass;
signed desktop and cross-client acceptance remain open. See
[unfinished output retention](work/partial-output.md).

Model access-program metadata now reaches the shared catalog and model detail
panel. Missing, empty and populated observations stay distinct. Installed-native
cold starts and fixed-ETag refresh pass; authorization remains native. See
[model access metadata](work/model-access-programs.md).

Code Mode helper execution and delayed MCP native history now have installed-native
qualification. Chief retains late MCP activity under its original terminal turn
without changing the active dispatch. Database reopen and source rejection tests
pass. Signed desktop and further cross-turn acceptance remain open; see
[Code Mode integration](work/code-mode.md).

MCP status now displays advertised capability and extension names independently
of tool discovery. Installed-native tests cover failed discovery, failed initialization
and fresh connections after restart. Enterprise OAuth fallback and trusted-project
authentication downgrades are rejected by the installed native owner. These changes
are unmerged; signed desktop acceptance remains open.

Permission catalog qualification confirms that each requested working directory
controls native profile availability. Duplicate configuration and thread warnings
now produce one notice per task and process; native owner and restart checks pass.

Global instruction refresh is native-owned. Installed-native tests cover live edits,
read failure, recovery, removal and cold resume. Native warnings now reach Chief
and ordinary history as display-only notices. Native bridge, owner persistence,
restart and history projection checks pass; combined signed desktop acceptance
remains open.

Caller turn triggers now distinguish actual user input, delegated instructions,
automatic wakes and capacity retries. Installed-native metadata and cold-resume
checks pass. GPUI animations now honor system reduced motion; signed desktop
accessibility acceptance remains open.

Voice settings now include per-start project resolution, native catalog selection,
versioned persistence and effective readback. The service and desktop picker are
connected. Installed-native bridge, concurrent-client configuration, restart and
GPUI interaction tests pass. Signed desktop/audio acceptance remains open; see
[voice settings](work/voice-settings.md). Protocol 2.63 is unmerged.

Large ordinary approvals now have exact database storage, privacy-selected 8 KiB
transport pages, complete client assembly and a bounded desktop reader. Protocol
2.59, request and desktop regressions pass. The installed native binary preserves
a 300 KB action when optional review falls back to explicit user approval; a
decline completes the turn. Large prescribed permission and policy replies now use compact
explicit decisions and retain exact native payloads. Actual desktop/service
acceptance and complete file-approval detail remain open. Native qualification
shows that pending file diffs arrive in `item/started` before they appear in
history. That evidence is now retained in the exact approval envelope; the installed
native bridge and coordinator test passes. Signed desktop acceptance remains open. Continue from [the implementation note](work/large-user-approval.md)
before advancing the source cursor.

Guardian failure records now have a restart regression that preserves absent risk
and authorization. The desktop labels denied actions as blocked without implying
a completed risk assessment. An installed-native test also confirms that a prompt
is saved before a pre-turn compaction error and remains unique after restart.
These changes are unmerged; desktop acceptance remains open.

Archiving an ordinary conversation now releases the byte budget used by its
removed live text. A regression reproduced missing output in the next task after
a full archived buffer. All 34 Conversations tests and strict GPUI Clippy pass.
This fix is unmerged; native desktop lifecycle acceptance remains open.

Provisioned macOS CLI packages add a shell launcher at `bin/codex`. The new
resolver recognizes the exact upstream launcher and package metadata, then sends
the bundle-native image through existing executable validation and attestation.
The reference snapshot now preserves the signed bundle context, and static/dynamic
path checks accept its exact main-executable relationship while retaining CDHash
validation. A real official alpha.16.3 package passes production attested control
startup through a symlink and relocated path. Four regressions and strict runtime
Clippy pass. Code Mode/helper/voice resource execution and full desktop acceptance
remain open. These changes are unmerged.

An isolated installed alpha.16 audio test confirms that native preparation replaces
empty tool audio with a text explanation and preserves adjacent text. The local
media reader already rejects empty decoded bytes; eight focused media tests pass.
Microphone and full voice desktop acceptance remain separate open checks.

Guardian now retains reviews within the native 8 MiB frame bound and serves large
actions in 8 KiB pages under protocol 2.58. Approval requires complete inspection;
changed review identities invalidate that inspection. Tests cover database reopen,
exact page reconstruction, stale identities and full submission without truncation.
An isolated installed alpha.16 test verifies a 300 KB action reaches Guardian and
its complete review events reach Decodex. Native request text is split into content
parts; reconstruction verifies the complete action. Full desktop, root/child and
compaction acceptance remains open. These changes are unmerged.

An installed alpha.16 WebSocket probe now verifies shutdown refusal for ten
new-work/lifecycle methods while reads and interruption remain available. The
admitted turn was interrupted and the host exited cleanly. Native queue/goal
suppression and combined desktop shutdown acceptance remain unverified. A focused
history-state regression rejects stale task/thread/account pages. Native voice
release resources belong to complete packages; binary version alone does not
prove helper availability or desktop audio acceptance.

MCP reconnect review confirms that native Codex owns expired-token recovery and
invalidates MCP runtimes after successful login. Existing local live/history
signals and scoped login tests pass (9 tests). Attachment adapter tests pass
(3 tests), and an isolated installed-binary probe verifies concurrent duplicate
adds, task isolation, restart readback, and repeated removal. Empty native threads
need persisted history before attachment mutations. Four resume regressions pass.
Full OAuth and resume/revert desktop acceptance remains open.

A further isolated native probe verifies attachment pagination across process
restart and rejection of another task's cursor. Saved disabled-plugin IDs survive
restart and default fork; an explicit empty list remains cleared after restart.
A separate native check verifies that referenced parents cannot be deleted,
rejected deletion preserves attachments, and deletion of the fork followed by its
parent removes their attachments across restart. The installed schema does not promise capability filtering. Historical fork
boundaries and per-task plugin controls still need separate acceptance.

Chief now records an exact native host-drain refusal as unsent when no context
injection preceded it. Input remains in history for a new send after reconnect;
uncertain writes keep their existing dispatch fence. Schema 40 permits cancellation
of a claimed capacity retry only with an exact refusal receipt. It restores the
original input's failed-turn identity and notifies a parent task when needed.
All 13 capacity tests and the migration test pass. Ordinary conversations now use
the existing positive non-submission evidence owner: evidence, local turn failure,
and a readable history record commit atomically. Database tests pass (111 unit,
5 restart integration). An installed alpha.16 WebSocket probe confirms that drain
rejects new turns while admitted work completes before a clean exit. Stdio does
not enable signal-driven graceful drain. Combined Decodex desktop acceptance
remains open.

Native history now displays public reasoning summaries and excludes raw reasoning
content. Projection and rendered GPUI tests pass. An isolated alpha.16 provider
fixture confirms completed summaries survive restart. During streaming, the native
timeline contains no reasoning item; summary deltas arrive as events instead.
Public summary events now retain ordered parts in DB schema 39. Completion replaces
partial text and late deltas cannot append to it. Protocol 2.57 and GPUI distinguish
these summaries from plans and assistant messages. Tests cover database reopen,
migration, voice handoff filtering, and exact native-item display deduplication.
Saved voice origins also filter later history pages. When local origins are absent,
the reader checks every native item page for that turn before showing a summary.
Cross-page regression and installed alpha.16 cold item-page probes pass. Full
retained-process reconnect and desktop acceptance remain open. The cutoff TUI
defaults summaries to `none`, superseding the earlier `detailed` default; Decodex
does not force that intermediate default. Source review is not delivery acceptance.

The local account API now retains the scoped `__oailb` routing cookie in memory.
Two cookie regressions and strict runtime lint pass. The lockfile now selects
rustls 0.23.45 to fix RUSTSEC-2026-0285, anyhow 1.0.103 and event-listener 5.4.2
to fix two unsoundness advisories. Runtime unit tests pass (554 passed, 31 ignored).
The next audit reports no vulnerabilities or unsoundness warnings. Four existing
unmaintained dependencies belong to the pinned GPUI/image stack. The yanked
chacha20 package remains in the lockfile but is absent from the resolved workspace
graph, including all features and targets. These maintenance risks remain visible;
this change does not replace the GUI dependency stack or suppress audit findings.

Chief model discovery now rejects results if the account revision or process
changes during the request, or the account becomes unavailable. A regression
covers a stable account, revision change and source loss. Native Codex owns
provider/auth cache identities and Apps tool-catalog propagation. Cold Chief
catalog invalidation and full cross-client acceptance remain open.

An isolated alpha.16 test confirms cold account/provider changes fetch the current
catalog and change its persisted identity. The same identity stays stable across
restart, but a fetch still occurs; offline cache reuse is not verified. This binary
ignores the upstream-only `model_catalog_url` setting. Do not treat that setting
as an available installed capability.

Installed Codex supports memory V2 readiness reads. Isolated warm/cold checks
verify typed results and threshold rejection. The current memory badge reports
only the feature flag; version selection and readiness integration remain open.

Automatic MCP user verification at this cutoff admits only the embedded Codex TUI
or the local named Codex Desktop host on supported devices. Decodex is not eligible
through its current identity. Direct local verification RPC support is separate;
backend registration and complete native verification acceptance remain unresolved.

Live voice now keeps the draft, attachments and task references visible. A GPUI
render and editing regression passes at narrow and wide widths. Native mixed-input
acceptance, cold voice labels and workspace file links remain incomplete.

Voice control connection failures now retire local media for the exact call.
The GPUI regression, native WebKit offer/stop check and strict lint pass.
Received transcript tails now survive control transport loss as resolved history.
Long delta streams retain the most recent 32 KiB at UTF-8 boundaries; accumulated
and single-chunk Unicode overflow tests pass.
Coordinator tests cover database reopen, partial write failure and repeated closure
without sending new input. Startup retry and combined desktop disconnect acceptance
remain incomplete. Final transcript writes now keep the corrected text until storage
succeeds; oversized finals preserve a UTF-8 prefix within the existing 32 KiB limit.
Fault-injection and database-reopen tests pass. Persistent storage failure during
transport teardown and subsequent deltas after a failed final remain unqualified.

Native terminal events now retain original start and completion timestamps with
the duration. Result and paginated recovery tests pass; timestamp presentation
and full desktop replay acceptance remain incomplete.

Strict-review notices now require the current ready native process in addition to
the exact running thread and turn. A regression reproduced the old unowned-process
acceptance; database and runtime tests cover rejection, deduplication and restart.
The native Guardian owns cached decisions, fresh review and authorization changes.
Concurrent native compaction and answer acceptance still need qualification.

Named task permission selection remains incomplete. A bounded native profile
catalog adapter and typed selection route now pass transport tests and an
installed-binary warm/cold test. The queue ACK does not prove application.
Source-bound permission observations now persist separately from model facts.
Transport snapshots now carry settings guards and reject stale writes before
coordinator event consumption. Late hydration cannot replace newer or invalidated
facts. Durable permission attempts now distinguish reserved, queued, unknown,
rejected and target-observed state. Only wire-current observations can settle a
selection. Pending attempts block task dispatch, tool upgrade and model recovery.
Database, observer and native warm/cold receipt tests pass. Runtime review/write
now binds account, task, current wire facts and complete native catalog before a
single durable selection. Protocol 2.53 exposes the review and selection route.
Controller tests cover stale sources, disabled profiles and lost acknowledgments.
Cold reconciliation now requires confirmed old-process death and complete current
owner facts. The GPUI profile panel passes rendered socket click tests, including
lost replies, disabled choices and draft preservation. Native warm/cold tests also
verify idle permission availability without an extra resume. The new controller also passes installed-native qualification through the
retained bridge. A fresh signed application bundle passes contract checks, but
the UI tool cannot select the isolated window; full desktop acceptance remains open. Live-turn reviewer
publication is a separate operation and does not close this gap.

The fixed upstream cutoff includes a macOS Secure Enclave verification provider.
Platform support does not establish device readiness or backend registration.
User verification RPCs exist in the installed binary, including cancellation.
An isolated unauthenticated status call returns `providerUnavailable`. At the
fixed cutoff, upstream enables verification elicitations only for its named
local desktop host and embedded TUI. Decodex is not eligible and advertises only
form support. A supported activation route, registration ownership and native
consent/cancellation acceptance are required before offering this capability.
The initial contract-only stub does not describe the final native implementation.

Explicit integration refresh now calls native `app/installed` with the exact
thread and `forceRefresh: true` after directory refresh. Directory metadata alone
cannot acknowledge a live tool refresh. A native error, unsupported method, or
invalid response keeps the operation unsuccessful without replaying earlier
mutations. Adapter and retained-bridge tests pass. The installed binary also
accepts reads and refreshes on an isolated thread with Apps disabled; hosted tool
replacement and failure retention still need native acceptance. The integration
panel now displays installed Apps with separate enabled and callable states.
Independent Apps errors do not hide MCP or plugin observations. This response
change requires local protocol 2.52.

The working branch includes signed commit `2ffa385c3b49efe6a4109de0fd7353fb64abd2c5`
after rebase onto main `79d15ef92cdb448abd67ca3972996ab304fea255`.
Account recovery changes after that commit remain uncommitted and unmerged.
Native command, account credential, same-UID client, peer event, concurrent claim,
source invalidation during launch, and database reopen checks have passed with
synthetic accounts and a loopback backend. Provider observation scheduling and
the complete GPUI click-to-effect flow are not covered by that combined fixture.

The Chief no longer forces the TUI realtime feature flag during thread creation
or voice resume. App-server realtime does not require that flag. The installed
native fixture passes with the flag disabled, including timeline persistence and
cold resume. Microphone and rendered WebRTC acceptance remain separate gaps.

Async questions now have per-work collapse/reopen controls and an explicit local
skip action. Schema 37 stores skips separately from native reply evidence;
protocol 2.51 binds skip to the displayed work, thread and question. Replay and
database reopen keep skipped cards hidden. Recovery, changed ownership and
pending replies reject the action. It sends no native input. Custom drafts now retain their editor across named-choice selection, and repeated
Enter events cannot submit an answer. Suggested answers now require their full
rendered option to be visible; resize and scroll regressions pass. Service profile
switches and disconnects now preserve each question editor. Changed native thread
identity discards the previous editor. Cold application drafts and connected
native desktop acceptance remain open. The private revisioned client draft
file store now passes save/reopen, concurrent-writer, corruption and path checks.
The bounded editor document and exact service-profile namespace also pass file
round-trip checks. GPUI background autosave and profile-scoped restore are now
connected. Fresh native history admits saved question editors; incomplete recovery
cannot create a duplicate editor record. Isolated save/reopen and writer-conflict
tests pass. Command dispatch now waits for a saved snapshot containing the original
command ID and delivery fence. Save conflicts prevent dispatch, and profile changes
cancel commands that have not started. Close-time flushing, conflict resolution,
and native desktop restart acceptance remain incomplete. Current GPUI tests pass
(335 passed, 5 ignored), as does strict Clippy. Temporary writer contention retries
without dispatch. Accepted or definitely failed commands immediately publish their
settled editor state. The GPUI Quit action now waits for saved drafts and cancels
exit on failure. Native AppKit termination now enters the same preflight without
replacing GPUI's delegate. A separate native process verifies cancel, continued
operation, accepted retry, and GPUI shutdown. Complete editor restart acceptance
remains open. Input entered before profile configuration now saves in an unbound
slot and moves atomically to the first profile. Later profile switches keep it
with that profile. The Keep both drafts action now resolves writer and startup
seed conflicts while retaining source-bound alternatives and later edits. The
current-profile alternative-copy list and explicit restore are now connected and
click-tested. Full-record export, confirmed removal, and capacity recovery are
implemented; unknown-delivery copies remain protected. Complete native editor
restart acceptance remains open. These changes are not yet merged.

Daybreak response decoding is covered, but its user control is not implemented.
The installed server supports a saved `daybreakEnabled` preference on persistent
threads, including initial `thread/start`, and a separate per-turn
`cyberAccessProgram`. The preference does not grant access or select a turn's
program. Integration must restore the native preference, preserve independent
fork choices, and keep running-turn settings unchanged. Account and model
eligibility, explicit selection, native persistence and UI acceptance remain open.

Safety buffering is an additional confirmed gap. The installed server publishes
`model/safetyBuffering/updated`, but Decodex has no consumer or retry UI yet.
Upstream requires explicit confirmation to interrupt and fork before resubmitting
the original input with the server-selected model. Keep this flow separate from
peak-capacity retry and account model fallback. Response streaming, completion,
and source changes must invalidate stale offers.

Collaboration-mode discovery is also missing. The installed protocol exposes
`collaborationMode/list`, but Decodex only checks its schema and has no production
catalog consumer or mode selector. Fetch optional choices from the current native
source and refresh on reconnect. Preserve the current task mode and editable input
when discovery fails. Explicit selection must apply the server mask and clear old
mode prompt overrides so the server supplies its current instructions.

Permission-profile discovery and selection are confirmed gaps. The installed
server returns project-specific profiles through permissionProfile/list with cwd.
Consume its catalog and configRequirements/read for the current native task scope.
The cutoff supports server-owned custom selection through thread/settings/update;
confirm published permissions and retain input on failure or uncertain replies.
The existing reviewer-only control and fixed new-task sandbox choices do not cover
this capability. Managed-policy enforcement was not exercised by the scope probe.

Historical misalignment reconciliation now retains precautions that stopped
realtime voice. Schema 34 stores this cause across native stop and process restart;
only an acknowledged explicit continuation clears it. Legacy precautions with an
unknown cause also require explicit acknowledgment. Database and runtime fixtures
cover successful/lost stop replies and reopen; microphone acceptance remains open.

Steering recovery now confirms exact native client-message receipts after lost RPC
replies, including cold history reads. GPUI retains the submission identity with
its service profile and uses a positive receipt query to settle uncertainty.
Layer tests cover different identities, profile switches, later draft edits and
attachments. An isolated installed-native live/cold fixture now carries positive
receipts through the real service query into rendered GPUI automatic recovery.
It seeds the uncertain UI state; it does not inject an actual transport loss after
a composer click. App-restart draft persistence remains incomplete. Absence of
pending rows is not proof of acceptance.

### Automatic model fallback is partly connected

The earlier manual plan described explicit fallback selection. That does not
cover the upstream behavior. At the fixed cutoff, ordinary model fallback is
automatic and separate from LunaReserve, which remains outside this task.

Source: `codex-rs/tui/src/chatwidget/backend_banners.rs`,
`codex-rs/tui/src/app/backend_banner_fallback.rs`, and
`codex-rs/tui/src/app/tests/backend_banner_fallback_tests.rs` in the official
`openai/codex` repository at the cutoff above.

| Required behavior | Current evidence and remaining adaptation |
| --- | --- |
| Select the first eligible backend fallback | Upstream requires the current model to equal `blocked_model_slug`, then selects the first different, visible catalog model in backend order. The Chief timer now consumes the account-bound list with the complete visible native catalog. Ordinary conversation integration remains missing. |
| Change only the exact current task | Upstream rejects stale recovery generations and mismatched active threads. The Chief owner checks the current account revision, native ChatGPT authentication, task, process generation, and settings guard before and after its durable reservation. An Accounts-panel selection cannot identify a task by itself. |
| Preserve task policy | Upstream preserves collaboration mode and permissions, keeps supported reasoning effort or uses the target default, and resolves the target service tier. Model permission defaults must not be applied during automatic recovery. |
| Wait for native acceptance | Upstream sends `thread/settings/update` and updates local state only after success. The installed `0.155.0-alpha.9.2` schema includes this method. The uncommitted narrow adapter and retained bridge now admit only model, effort, and service tier. Installed-native tests confirm Default/Plan mode, instructions, and permission preservation after `thread/settings/updated`, with no extra inference or global config change. The Chief timer now calls the adapter. Its full automatic trigger-to-UI flow remains unverified. |
| Preserve user intent and input | A manual selection queued during the update wins afterward. Recovery does not write global defaults, replay a turn, or automatically return from an ordinary fallback when usage recovers. Failed or unsupported updates leave the existing selection and recovery options available. |

Cold resume and ordinary continuations now preserve the native task model and
effort. Explicit per-message choices still override these settings. The installed
binary test covers Default and Plan, process restart, and the model and effort in
the next inference request. The same native fixture also verifies omitted service tier in both modes. Fast disabled preserves the current tier, including an unset tier; unknown Fast support does not authorize a new tier. These checks do not prove automatic fallback delivery.

The composer now submits only explicit next-message changes for an existing task.
An unchanged model, effort, or tier inherits the native setting. Legacy complete
execution selections retain their meaning. Pending choices are scoped to the task
and service profile. An acceptance clears only the captured choice revision, so
later user edits survive. Native model reads cannot replace pending choices or
an exact-ID draft. The open model picker refreshes its task observation, and a
read invalidated by a native settings publication is discarded.

Installed-native evidence covers an effort-only message after capacity recovery:
the request keeps the recovered model and uses the newly selected effort. This
does not yet prove the ordinary conversation composer or automatic fallback owner.

Peak retries now use the requested selection saved with the exact native turn
acknowledgment. An ordinary turn reads and binds the current native model and
effort; an explicit per-message selection takes precedence. A newer selection
cancels the old retry. A connection-local settings guard also rejects stale
retry writes before the owner processes the notification. An installed-native
test verifies an actual overload, process and database restart, the same selected
model and effort, and no duplicate user input in the retry request.

This receipt describes the submitted choice, not inference telemetry. The
installed Turn schema has no model field. Native-owned turns and lost local
acknowledgments have no such receipt yet; these cases must not fall back to the
coordinator defaults. Their complete automatic-continuation adaptation remains
part of the manual catch-up.

Recovery reservations now survive restarts and concurrent clients in the existing
event journal. Native replies and matching later settings publications are separate
immutable records. A queue acknowledgment does not prove publication. Only a live
notification from the original owned process can record the full target settings;
a start/resume readback cannot settle the pending reservation. This establishes
observed state, without asserting that the recovery request caused it. The Chief timer now reserves and sends the guarded update. Pending native events
block recovery until the normal reducer consumes them, including a notification
that arrived before the current settings guard was captured. Complete automatic-flow
acceptance, supersession handling, and the ordinary-conversation path remain unfinished.

Implement and verify this task-scoped path before claiming model recovery is
complete. Keep peak `serverOverloaded` retry separate from usage-limit fallback.

## Historical checkpoint: 2026-09-19

Decodex base: `009b49ca4f17ebaef5f096fa237a9c04936b746e` (PR #1352).
This includes PRs #1346–#1351 and the native integration/OAuth delivery.
The task-usage estimate work below builds on that base. Historical sections retain dated source and test
evidence; they do not override this checkpoint.

| Capability | Included implementation | Remaining work |
| --- | --- | --- |
| Models | Paginated native model catalog, efforts, Fast, image support, access/upgrade/retirement notices and exact turn speed. PR #1348. | Account-transition acceptance and remaining model-related commit dispositions. |
| Replies and questions | Structured async questions, drafts, exact native replies, recovery and resolution. Nonblocking timing and provider resolution are included. PRs #1344–#1346. | Broader native acceptance across account/process transitions. |
| Misalignment | Explicit review and native continuation override with exact turn checks. PR #1347. | Include in the overall cross-feature acceptance pass. |
| Inputs and resources | Composer images/path references and native task attachment associations with cross-client readback. PR #1350. | Remaining upstream input/resource capability dispositions. |
| Execution | Per-message model, effort and Fast; bounded same-model serverOverloaded recovery. | Live turn/settings/update has no current UI consumer; record final applicability assessment. |
| Observations and recovery | Token usage, compaction, approval details and native agent activity. This change adds account-scoped task estimates. | Remaining account/usage and analytics capability dispositions. |
| Voice | Agent subscription voice and native media host. | Assess remaining realtime changes against actual consumers. |
| Plugins/MCP | Native typed elicitation and scoped replies (#1349/#1351); PR #1352 adds repository plugin status, independent MCP runtime/auth/discovery states, explicit reconcile/reload and native OAuth. | Remaining resource/error contracts and commit dispositions. |

The resumable consecutive full-diff review covers 76 of 1,569 commits in
`a397079287e6638b39dda329835350d93222681f..595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Last reviewed: `18937b226524164546e7328a2ed47c0d52536e0a`.
Next: `ffad92234000c3c0cf4b48cbf1e92c96b0ab5742`.
The other 1,493 commits have not all received consecutive full-diff review;
grouped capability findings below cover portions of that remaining range.
The lower boundary is historical, not a certified earlier audit. A grouped
capability result does not advance the consecutive cursor.

For this integration change, all tests in the four affected packages pass,
including the rendered explicit-authorization interaction and real local
AgentClient transport. All-target/all-feature Clippy passes for those packages.
The installed 0.155.0-alpha.9.2 binary passed isolated two-client integration
reload and local OAuth/MCP acceptance. These checks prove this adaptation's
boundaries, not completion of the overall upstream audit. Automation stays paused.

## Historical review: 2026-09-17

## Review boundary

Initial Decodex base: `579b8d74fe57b2915e3695ad76fd519ea7bbf4f5`.
Follow-up base: `d0b20dbc32bcfd3952a938a74aadb4544a3fc981` (merged PR #1337).
Official Codex main: [`c5d079470eeaf9502080faa0697aade481242081`](https://github.com/openai/codex/commit/c5d079470eeaf9502080faa0697aade481242081).
Stable release: [`rust-v0.154.0`](https://github.com/openai/codex/releases/tag/rust-v0.154.0),
commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`.
Installed test binary: `codex-cli 0.154.0-alpha.6.2`.

This review establishes a current integration baseline from source, official
experimental schema exports, the installed binary's generated schema, and tests.
It does not certify that all 1,445 historical commits after the old August 18
cursor were individually reviewed. That cursor was not accepted as proof of
current compatibility. The current Conversation and Agent implementations
supersede the old Quick Task implementation and PRs #1300 and #1301.

## Changes delivered

| Upstream change | Decodex consequence | Adaptation and evidence |
| --- | --- | --- |
| [`c62d191c4c`](https://github.com/openai/codex/commit/c62d191c4c8c0cab7045fca6efc399197334bb6c): `thread.rs`, `turn.rs`, thread processor and resume/fork tests | New `disabledPluginIds` response fields fail the strict Conversation start/resume decoder. | Accept the actual string-array field, default it when omitted by older servers, and reject malformed values. Upstream main only relative to the tested stable release. The field does not yet enforce plugin filtering. |
| [`91d54f1667`](https://github.com/openai/codex/commit/91d54f1667e627538db9d44d2ce88a260b4213b0): resume protocol, thread processor, persisted/legacy collaboration-mode tests | The new `collaborationMode` response field fails Conversation resume decoding. | Decode the optional typed mode/settings object, including snake-case settings. Preserve existing model, effort, cwd, and permission checks. Upstream owns restoration of saved mode. Main only relative to the tested stable release. |
| [`5cb7a35de9`](https://github.com/openai/codex/commit/5cb7a35de938e2475e5c1c088f111915008fd100), [`d132b69219`](https://github.com/openai/codex/commit/d132b692199c53c085c7b2cbec3c44e2dc5cf277): native history list APIs, thread processor, `thread_read.rs` and `thread_resume.rs` tests | Agent completion and recovery load the whole thread and use deprecated full-history hydration for paginated threads. | New Agent/worker threads select paginated history. Resume requests exclude turns. Exact result reads page turns and items, preserve item order and metadata, and reject repeated cursors, wrong identities, and exhausted bounds. Existing legacy threads retain their supported read path. This capability is released and was tested with the installed binary. |

The page reader has a 60-second deadline, 128-page limits, and an 8 MiB aggregate
page budget. A missing turn remains missing. An incomplete read returns an error;
it does not invent a complete result or grant dispatch replay authority. Agent
still records positive terminal evidence separately from result-read failure.

## Current integration coverage

| Surface | Sources and current conclusion |
| --- | --- |
| Initialization, transport and process ownership | Current `AppServerClient` and account-process bridge were compared with official request/notification exports. Stdio and the initialize/initialized handshake remain supported. Multiplexing, exact response IDs, event overflow, disconnect handling, and explicit process ownership have local regression coverage. |
| Thread start/resume/read/list/archive | Compared current and installed experimental exports for these methods and nested Thread/Turn definitions. The response additions above were the uncovered start/resume fields. Current Decodex already accepts project, model, effort, originator, environment and Daybreak metadata. Read/list projection tolerates additive fields. |
| Turn dispatch, steering, interruption and recovery | Current turn request/result shapes remain compatible. Agent preserves independent thread and active-turn identity and does not replay uncertain submissions. Pagination tests cover missing exact turns and cross-turn result rejection. |
| Authentication | `ChatgptAuthTokensRefreshParams` and response shapes are unchanged in installed, stable and main exports. The copied login source has an explicit baseline, `9392c3fa5bcda342b5b96a1a04d67b2f781617c2`. Comparing its four cited source files with current main found only an added optional Bedrock storage field and its `None` initializer. Browser/device flow and PKCE logic are unchanged in that bounded source scope. Current local refresh classification already handles HTTP 400 `invalid_grant` as rejection. |
| Account limits and model discovery | Account responses accept additive metadata. The merged Agent now implements retained-process model discovery; see the current checkpoint. Account quota display uses the direct account API, not the new app-server usage capability handshake. See the open gaps below. |
| Sandbox and approvals | Current installed/main experimental shapes used by Decodex remain compatible. The retained bridge adds only the two read-only history methods. Account mutation and unowned approval responses remain rejected. Approval and user-input requests retain exact IDs and require an explicit owner response. |
| Native collaboration | The schema check alone missed semantic losses: four v2 tool names, interrupted tool calls, and completed child activity normalized to Unknown. This follow-up adds their typed classifications. Agent still owns independent work threads; a native child completion does not resolve Agent work. |
| Messages, usage and compaction | The earlier patch preserved stored metadata but missed delivery timing. This follow-up records async assistant messages before terminal completion, public token counters, and completed compaction observations. The merged Agent also has pending question forms and exact-turn steering; structured async-message UX and timeout behavior remain separate. Internal raw-response usage metadata is not interpreted as price. |
| Removed or optional features | Decodex has no `thread/rollback` consumer. Its removal does not require a local compatibility alias. TUI-only controls, new provider onboarding, remote-control services and optional plugin-management APIs do not require ports into the active integration. |

The principal source owners are `codex-rs/app-server-protocol/src/protocol/v2/`,
`codex-rs/app-server/src/request_processors/thread_processor.rs`,
`codex-rs/app-server/tests/suite/v2/thread_read.rs`, and the four login files named
in `crates/decodex-account-login/THIRD_PARTY_NOTICES.md`.

## Validation

- `cargo +stable test -p decodex-codex -p decodex-runtime -p decodex-account-login --all-targets --all-features`: passed, including pagination, strict metadata, durable recovery and account tests.
- Clippy for `decodex-codex` and `decodex-runtime`, all targets/features: passed with warnings denied.
- `official_schema_supports_current_consumers`: passed separately against installed, stable-release and current-main experimental exports. Set `DECODEX_REVIEW_SCHEMA` to the export directory and explicitly run the ignored test.
- Installed app-server initialization: passed.
- Installed real two-thread smoke: passed. Each test thread completed a small read-only request; native pages returned its exact assistant result. Both created threads were archived. The initial ephemeral-thread attempt was rejected because Codex does not support history pages for ephemeral threads; the corrected test uses persistent test threads.
- CLI/service build, local database gate and vNext architecture tests: passed.
- Repository-pinned formatting and `git diff --check`: checked for the changed code.

The user has retired the website. Site dependency and site build checks are outside
this maintenance scope. The earlier site audit failure is not an outstanding
Codex adaptation. OpenWiki remains unchanged. An additional account-login
architecture check still contains two obsolete source-text assertions for
`AccountLoginRequest::Status` and `AccountLoginRequest::Cancel`; the active login
runtime tests passed. No live product database or account configuration was changed.

## Behavior follow-up

The follow-up compares the old claimed snapshot with current source contracts,
request processors, and active Decodex callers. These decisions are distinct from
an exhaustive review of each historical commit. The old consecutive commit cursor
remains unverified; do not advance it to the current head on this evidence alone.

| Capability and upstream evidence | Actual Decodex behavior and decision |
| --- | --- |
| Async messages and structured questions: `fb356f3d2c`, `2c79ee6dac`; core `tools/handlers/request_user_input_async.rs`, `tools/spec_plan.rs` | Upstream emits an `agentMessage` with `delivery: async`, rendered text and structured questions, then continues. It is model-catalog gated and is not a pending JSON-RPC request. Fixed immediate saved/displayed messages with exact thread/turn ownership, deduplication and restart persistence. Ordinary text replies still queue for the next Agent turn; interactive reply delivery remains open. |
| Token usage: `5f79a92e39`, `2c4a95736b`, `e017e93ace`; protocol `thread.rs` and `thread_data.rs` | Added durable public `thread/tokenUsage/updated` observations, live history display and terminal receipt metadata. Show last **response** counters separately from cumulative **thread** counters. Missing context capacity stays unknown. No fabricated context occupancy, costs or quota readiness. The two-thread installed-server test now requires valid, same-turn usage before completion. |
| Compaction and retained answers: `5971d42847`; core compaction and item projection | Codex owns compaction and retention of verified answers. Show completed compaction in Agent history without creating pending work or triggering a new model turn. No Decodex-side rewrite of native history. |
| Approval context: `9c9675d3d0`, `eb078b4f44`; protocol `item.rs`, `permissions.rs` | Fixed projection of `kind: writeStdin`, additional/network permissions, proposed policy amendments, nullable decisions, and permissions-request cwd. Old missing kind defaults to command, matching upstream. Exact callback/event ownership and explicit responses remain required. No path normalization of target-native cwd. |
| Misalignment and rate-limit errors: `7276d67081`, `e0c727de04`; `thread_data.rs`, `shared.rs`, upstream `misalignment_policy` tests | Display saved error message and substantive explanation. Do not automatically submit the suggested continuation. Agent's generic terminal error storage accepts new classifications. A purpose-built continuation UI and auth-recovery progress display remain open. |
| Native agent v2: `4fa6ad1730`, `b705b6b076`; protocol `item.rs` | Fixed `sendMessage`, `followupTask`, `interruptAgent`, `listAgents`, interrupted tool status and completed child activity classification. Use `agentThreadId` for the activity target and record the containing thread separately. Source emission in core `session/mod.rs` and `multi_agents_v2` shows that the containing thread can be the initiator or a peer; it does not prove a parent edge. Preserve redaction. These are native actor facts, not Agent work acceptance. |
| Model discovery/access programs: `e3a52b87b2`, `94967e03e5`; `catalog_processor.rs`, `model_list.rs` tests | **Partly implemented.** Retained-process pagination and GPUI model/effort/Fast/image controls are included. Retirement guidance, access programs and account-transition acceptance remain to assess. Do not advertise an absent/null access program as denied or granted; do not auto-select a different model. |
| Thread attachment records: `3319d9b296`; `thread_attachments.rs` processor/tests | Main-only in the reviewed exports; absent in installed/stable. Stores JSON by thread/type/identity, supports unloaded reads and idempotent add/remove, and can be unsupported by the backing store. This is a useful future PR/artifact association API, **not file upload or model input**. Decodex has no corresponding attachment product owner; do not mirror its work database into Codex. |
| Image file references and standalone tool output: `7b8b17b97a`, `e56e4922eb`; protocol `turn.rs` | Image inputs now accept `fileId` as an alternative to inline URL. Text input remains supported. Agent now supplies localImage and non-image path references. Native fileId and externally supplied standalone tool results remain separate unimplemented flows. They require an actual input/result flow, not merely an unused allowlist entry. |
| Plugin reconciliation: `bfa9646787`, `5918c743f3`; `plugins/reconcile.rs`, `plugin_reconcile.rs` tests | Installed and stable expose reconciliation. It reports changes in this pass, including removals and failed materializations; it is not proof of runtime readiness. Upstream refreshes loaded hooks. Decodex has no plugin settings/reconcile UI; this remains a product gap rather than a port of upstream bundle internals. |
| Disabled plugins and app tool exposure: `c62d191c4c`, `0ec375eb70`, `a6d4741d39`; `thread.rs`, `turn.rs`, `config.rs` | Main adds saved disabled IDs and per-app/per-account settings. The disabled-ID contract explicitly says it does **not yet filter capabilities**. Metadata decode is fixed in #1337. Do not ship a misleading disable switch. Future settings must distinguish saved preference, actual filtering, reconciliation and loaded runtime readiness. |
| MCP state, UI and elicitation: `343074d420`, `8f31b64c7f`, `7a6f469dcf`, `b71af39fe6`, `eec4a23cb1`, `097825f75a`, `a1dc95d5af`; `mcp.rs`, `item.rs` | MCP discovery failure differs from an empty catalog; runtime state differs from advertised capabilities. App UI metadata and scoped resources require a renderer/resource owner. Agent currently exposes four pending request methods; MCP forms and native verification are not among them. This is an open interaction gap, not proof that MCP forms work. |
| User verification: `ad931a45b2`, `555b82afa9`, `82d4a98912`, `7b491281c8`; `user_verification.rs` | Experimental enrollment/status/verify/delete/cancel and public-key metadata require a native verification UX. No Decodex consumer exists. Do not auto-enroll, auto-answer, or enable opt-in transport as a compatibility fix. |
| Live settings: `9695e71519`, `9112564114`, `ed42068c45`; `turn.rs`, turn processor | `turn/settings/update` affects later captures in one matching live turn; `applied` does not prove another inference occurred. Per-turn tier overrides do not change thread defaults. Per-message execution overrides are now implemented. Live model/effort/reviewer/tier controls remain a product gap. |
| Account usage: `577a4fcd06`, `5037919777`, `79b04f1ab5`, `a4354e2d27`; account processor | New usage-read capabilities default false. Ordinary usage permission is account/user-validated and must not be inferred from percentages. Decodex does not advertise Luna Reserve fallback. Existing direct quota display remains separate; adopting fallback or backend upsell needs account-bound evidence and is not implemented here. Workspace routing is upstream-owned; accepting metadata does not select another endpoint. |
| Managed config/provider policy: `1aaa453ce2`, `a20092a7a2`, `ce950dcf26`, `b27a6321fa`; config and turn processor | Upstream enforces provider definitions/login restrictions and adds developer/application requirements. Decodex does not write these settings. Admission errors must remain visible; empty allowed-login methods do not mean unrestricted. Browser/computer policy additions apply to optional clients Decodex does not implement. |
| Thread identity, history and provenance: `5cb7a35de9`, `d132b69219`, `986ff1cc7c`, `728cb12fe5`, `2b554fd3f9`, `196964ef10` | #1337 adopts exact paginated history and preserves existing strict start/resume identity checks. Configured thread model is not per-turn telemetry; environment selection is not connection health; root-turn attribution does not replace execution turn identity. |
| Other native-owned changes: realtime timeline/attachment, project recency, memory v2 readiness, interrupt hooks, rollout compression, Bedrock setup, Windows sandbox implementations, feedback prompt hash, Guardian attribution | Reviewed current public contracts and local callers. The merged Agent includes realtime voice; reassess that surface against its native consumers. Native project, memory-admin, Bedrock and Windows setup clients remain separate applicability questions. Core memory/hooks/review execution stays in the installed Codex process. Compression acknowledgment is not completion. No client call is added without a corresponding product behavior. |
| Removed/deprecated controls: rollback, detached review, personality | No active rollback/detached-review/personality control requires a compatibility alias. A separate review should use a separate thread when that product flow is added. |

## Remaining adaptation queue

Keep two independent queues: consecutive upstream diffs and capability acceptance.
Start from fresh main and check merged changes, open PRs and known related tasks.
Reuse the implementations in the current checkpoint before adding code.

1. Complete question behavior: preserve isBlocking, implement nonblocking timeout
   and empty-answer skip, and validate structured async replies in the exact turn.
   Preserve uncertain steering acknowledgments without replay.
2. Validate model discovery across account transitions and inspect access/retirement
   metadata and optional fields. Do not rebuild the existing catalog or picker.
3. Assess native file inputs and artifact associations beyond existing localImage
   and path references; keep stored JSON attachments distinct from model input.
4. Assess plugin/app settings, reconciliation and MCP forms/resources against the
   installed release. Prove effective behavior before showing an active control.
5. Assess live turn settings and auth recovery, then validate observations and
   capacity recovery with the merged UI, account transitions and voice consumers.

For each item, record source/commit, consumer, implementation status, evidence and
one exact next action. Keep historical review advancing when product work overlaps
another task. Do not mark the integration caught up while either queue is open.

## Follow-up validation

- All-target/all-feature tests for Codex, database and runtime packages pass.
- Clippy for those packages passes with warnings denied.
- Real installed dual-thread smoke passes, including same-turn usage before
  completion and exact native history reads. It does not exercise every optional
  model-gated async tool or main-only API.
- Database migration 15 adds two history indexes. A disposable version-14 upgrade
  test preserves existing messages and pending disposition. The database gate
  passes at schema 15 with 47 tables. No existing migration was rewritten.
- Regression tests cover observation deduplication, persistence after reopen,
  absence of work/wake side effects, visible async text, terminal usage, approval
  context, failure explanations and native v2 event classification.
- vNext architecture checks pass. Website checks are excluded by the user's
  retirement decision, not reported as passing.

## Decodex model-capacity recovery

The user requested automatic recovery from peak-time model capacity errors, not
Luna Reserve or account-quota fallback. Official main `b0659c53865dd48b0cd69c454368cea3980017cc`
maps `server_is_overloaded` to `serverOverloaded` and treats it as non-retryable
for ordinary sampling. This is a deliberate Decodex behavior, not a claim that
ordinary upstream turns already retry it.

Agent and its worker threads now schedule up to three continuation attempts,
with waits of 15, 30 and 60 seconds. The existing 15-second service tick can add
up to one tick of delay. Work must still be open. Both the terminal event and exact saved turn must report
a failed `serverOverloaded` result. Missing history, quota exhaustion, other
errors, interruption and uncertain turn submission do not authorize this retry.

Retries use the same account process, thread, model, effort and permissions. They
send a continuation instruction against native saved context, not a second copy
of the original user input. Completed work stays in that context. Original inbox
delivery receipts move to the acknowledged continuation and are handled only
after it completes. Worker capacity waits do not wake Agent with a premature
worker result; final failure or explicit cancellation can report that result.

Migration 16 adds the durable retry state, count, deadline and exact failure event.
Claiming a retry and fencing dispatch is atomic. A lost acknowledgment stays
unknown and does not trigger another attempt after restart. Fresh Agent input
takes precedence over a due retry. Explicit work judgments or a new dispatch
cancel pending retries. The history view shows a cancel button and reports when
the three attempts are exhausted. CLI cancellation uses:

```sh
decodex agent cancel-retry --work-id WORK_ID --event-id EVENT_ID
```

The new cancellation command uses local protocol 2.17; the desktop client and
service must run the same protocol version. The website and OpenWiki are unchanged.

## Voice history follow-up

The merged voice consumer still requested `thread/read` with `includeTurns=true`
at call start and recovery, although Agent threads use paginated history. Voice
now reads the newest native turn header as its baseline, pages newer headers back
to that exact baseline on recovery, and loads each terminal turn through the
existing exact-turn item reader. Missing baselines, duplicate turns and incomplete
pages fail explicitly. Legacy history remains supported. No audio or instruction
is replayed, and native WebRTC sideband reconnection remains owned by Codex.

Validation: nine native history fixture tests and 42 Agent behavior tests pass;
Clippy for Codex/runtime all targets and features passes with warnings denied.
These checks do not simulate a live audio disconnect.

## File approval detail follow-up

Pending file approvals now load their exact native thread, turn and item through
the paginated history reader. The request panel shows source paths, move
destinations and patch text. Native path spelling is preserved. Missing details
have an explicit fallback; bounded output has a truncation label. The application
rechecks request ownership after the read so a resolved or ended request cannot
be enriched as an active approval. Decisions still use the original event ID.

The existing tool detail reader also uses exact-turn pagination. Validation:
three detail fixtures, request projection and stale-request tests, runtime
Clippy across all targets/features, and GPUI compilation passed. No live approval
or visual acceptance is claimed. Terminal misalignment admission and nonblocking
question timing remain open adaptations.

## Nonblocking question follow-up

The pending-question projection now retains validated `isBlocking`. The GPUI
request panel follows the upstream fixed policy: 60 seconds of grace, then a
60-second countdown. Only explicit `isBlocking: false` enables the timer;
missing metadata remains blocking, and deprecated `autoResolutionMs` is ignored.
Keyboard or mouse interaction stops automatic resolution for that request.
Expiry sends `answers: {}` through the original pending event response route;
it does not select an option. Automatic submission requires a fresh pending
snapshot and is attempted once, with no retry after uncertain acceptance.

Timers retain their stopped state across selection changes and reset with the
service profile. Countdown text follows the controls so removing it on mouse
down cannot move an option before the click completes. Three GPUI timing and
interaction tests and the runtime pending-request projection test pass. This
implements native request-user-input callbacks; structured asynchronous agent
message questions remain separate work.

## Native request resolution follow-up

Agent now consumes `serverRequest/resolved`. A notification must match both
the current connection's typed JSON-RPC request ID and the original thread.
The matching pending event is resolved, its response authority is removed, and
the UI can no longer offer it. Duplicate, unknown and wrong-thread notifications
do not resolve another request. The receipt identifies provider resolution;
it does not claim that Decodex sent an answer or that the work is complete.

Validation: 43 Agent tests pass, including colliding item IDs, typed request
identity, wrong-thread and duplicate notifications, and rejection of a response
after native resolution. The existing database receipt test also checks that
request resolution does not change work judgment.

## Structured asynchronous question follow-up

Reviewed upstream `dbf478850fb84b7d32b4b9d4c4df43aa8539be83` and
`2808a9c348ee90a6fc94aee1570dd3fdf2c0b021`, including the final question state
and reply implementation at `595cc91e8cbb1c2ca822d0311dcf12709410c582`.

Agent projects structured async agent questions separately from request callbacks.
Each card has the upstream item/index identity, suggested options, free text and
explicit submission. Selecting an option does not submit it. Drafts are isolated
by work and question identity. Replies use the native desktop-compatible envelope;
history renders readable questions and answers. Running work receives exact-turn
steering. Idle work starts a turn on its original thread, including old managers
whose next ordinary dispatch will upgrade tools. No answer wakes a parent manager.

SQLite migration 25 preserves questions, answer tombstones and pending recovery.
Native replies, including other-client replies, dismiss only matching questions.
A new ordinary prompt retires earlier questions without allowing replay to reopen
them. Upgrade and reconnect recovery read exact native history with turn/item
pagination. Incomplete recovery withholds cards and retains drafts. A live remote
prompt must appear in history before its recovery marker is removed. Persisted
uncertain submission evidence prevents another answer attempt after restart.

The TUI's 30-second async expiry applies only to collapsed questions. Expansion
stops it. Decodex displays expanded cards and does not submit or expire them on a
callback timer. The separate nonblocking callback policy remains unchanged.

Validation covers durable reopen, legacy and paginated history, incomplete reads,
remote prompt replay, exact running/idle worker delivery, rejection/disconnection,
restart fencing, and rendered option/button/keyboard interaction. These are local
protocol and native GPUI fixtures; no live model-generated question session is
claimed. This capability delivery does not close the remaining model metadata,
misalignment, attachment, plugin/MCP or broader upstream review work.

## Misalignment precaution and explicit continuation

Reviewed the final TUI `chatwidget/misalignment_policy.rs`,
`app/misalignment_policy.rs`, and app-server `protocol/v2/turn.rs` at
`595cc91e8cbb1c2ca822d0311dcf12709410c582`. A nonretrying
`misalignmentPolicyViolation` must stop ordinary input. The supplied findings
may permit an explicit acknowledgment and continuation; they never authorize
an automatic retry.

Migration 26 preserves the exact thread, failed turn and findings. Ordinary
messages, steering, provider approval responses, automatic dispatch and new voice
sessions cannot bypass the precaution. Undelivered user input is retired and
pending capacity retries are cancelled. Active voice is retired locally before
the native stop request; late SDP cannot reactivate it after a lost stop response.
Reconnect recovery inspects the latest native turn, including idle threads that
failed before this upgrade. Older historical failures do not pause newer work.
Long terminal error summaries retain their provider error classification.

Protocol 2.28 exposes a source-bound review separately from transcript pagination.
The interface first shows the exact continuation request as a quoted string and
the full bounded findings. A separate acknowledgment submits their digest. The
host rechecks current native findings, claims the continuation durably, and sends
`turn/start` with the supplied text and experimental `responsesapiClientMetadata`
containing `misalignment_override`. Only acknowledgment of a different new turn
clears the precaution. Rejection retains it; uncertain acceptance cannot be
replayed after restart. Explanation and steer limits follow upstream, and a steer
is never truncated into a different instruction.

Local fixtures cover persisted pause and continuation claims, stale/retrying
errors, missing acknowledgments and reopen, changed native findings, latest-turn
recovery, blocked approvals, and voice retirement with a lost stop response.
Rendered GPUI tests cover separate review and acknowledgment clicks and stale
review rejection. This evidence does not claim a live provider-generated
misalignment session or completion of the remaining upstream capability audit.

## Model catalog metadata and explicit turn speed

Reviewed `protocol/v2/model.rs` and `protocol/v2/turn.rs` at
`595cc91e8cbb1c2ca822d0311dcf12709410c582`, plus
`78d4d983d3380fa92aa17fbe9e4f9b737b6a5223` (reasoning-effort update support)
and `ed42068c45` (turn-scoped service tier).

Existing model discovery already reads all native catalog pages on the retained
Agent process. The adapter now retains bounded availability text and suggested
upgrade/retirement metadata. The model menu displays these notices without
changing the selected model. Optional legacy upgrade metadata remains supported.
A catalog read captures its process generation and discards its result if that
process is replaced before the read finishes. Protocol 2.29 carries these notices.

Configured messages send explicit `serviceTierForTurn` values: `priority` for
Fast and `default` for standard speed. The existing `serviceTier` field remains
for older app-server compatibility. New servers therefore cannot interpret an
explicit Fast-off choice as an instruction to inherit a priority default.

The explicit `supports_reasoning_effort_updates` capability in commit 78d4d983d3
belongs to the native core's configuration-update/history pipeline. Decodex sends
request-level effort and does not inject those history items, so the native core
owns that adaptation. `availableAccessPrograms` advertises explicit cyber program
selection, not generic model availability. Decodex's current general-purpose
Agent does not select a cyber program; discovering one must not silently opt in.
The existing thread-level service-tier contract remains valid alongside the new
per-turn override. These dispositions do not claim a completed review of unrelated
model or account changes.

Validation includes paginated catalog reads, bounded and missing notice metadata,
legacy upgrade fallback, unchanged model selection, rendered notices, and native
request fixtures for explicit priority and standard speed. No model inference is
needed to read the catalog.

## MCP elicitation forms and explicit approval replies

At fixed upstream snapshot 595cc91e8cbb1c2ca822d0311dcf12709410c582, the
app-server MCP contract permits standalone requests with a null turn ID. A request
still belongs to one exact thread and one live native connection. Decodex now
projects these requests while that thread is idle; requests with a turn ID must
match the running turn. Private device challenges are excluded from projections.

The GPUI request panel supports primitive forms, explicit booleans, numeric input,
single choices and string-array choices. Wire values remain separate from labels.
Defaults do not become submitted answers. Required fields, primitive types,
advertised choices, lengths, counts and numeric bounds are checked in the client
and again against the original persisted schema before native response authority
is consumed. Unsupported schema assertions show an unavailable-form explanation.
String format is an annotation, as in the upstream TUI primitive text controls;
this is not a general JSON Schema validator.

Message-only approvals support only the session/always scopes offered by the
request. Tool suggestions retain the native empty-object response and do not gain
persistent approval. URL verification has separate open and completion actions;
opening a link does not report successful verification. Buttons support keyboard
activation. Device-authenticated openai/userVerification acceptance remains
unavailable because Decodex does not own the device proof producer; it offers
decline/cancel rather than fabricate a proof.

Native serverRequest/resolved retires the exact live request. Reconnection does
not restore response authority from persisted requests or reuse an old event when
the provider reuses an RPC ID. Validation rejection preserves the live request;
an uncertain transport response is not replayed. Protocol 2.30 prevents older
clients from treating the new MCP request projection as a command approval.

The source authority is app-server-protocol/src/protocol/v2/mcp.rs,
protocol/src/mcp_approval_meta.rs and
tui/src/bottom_pane/mcp_server_elicitation.rs under upstream codex-rs/.
Runtime fixtures cover idle-thread projection, original-schema response checks,
explicit false, one-shot replies, resolution identity and reconnection fencing.
Rendered tests cover offered persistence, separate URL confirmation and invalid
local-file links. These tests use a controlled native transport and GPUI test
platform; they do not claim successful authentication with a live external MCP
service. Plugin effective settings, resources and attachment work remain open.

## Attachment contract audit in progress

Commit 3319d9b296bba4cad340ffa997d216d95f601992 adds durable native thread resource
associations. The add/list/remove endpoints do not load a thread or change its
conversation history. Identity is the tuple of thread ID, attachment type and
identity key; repeated add returns the existing payload rather than updating it.
Creation/deletion notifications follow the successful response, while repeated
add/remove do not emit another update. Unsupported stores return an error; that
must not be presented as an empty attachment list. These facts are confirmed in
request_processors/thread_attachments.rs at the fixed upstream snapshot.

Before this adaptation, Decodex had local composer attachments but no native
resource-association consumer. Its localImage input and file-path text are separate from these new
endpoints. The native generic API does not reserve an attachment type or payload convention.

Commit 7b8b17b97a5f08f088852e8bc9ae388cff38c714 adds image fileId references alongside
URL inputs. Decodex localImage submission remains valid. The native history reader
keeps complete item JSON instead of rebuilding image content. A transport fixture
now verifies mixed URL/fileId input order, original/low/high detail, and image-only
messages across item pages. This proves preservation through history reads, not
image preview or the ability to upload files to obtain native file IDs. Native
image generation/editing owns resolution and unsupported recent-image-window
errors; Decodex must not substitute a local or older image for a file reference.

The current adaptation adds a native association client and a task resource panel.
The native thread store is the only persistence owner. Reads preserve pagination,
reject incomplete pages and distinguish unsupported storage from an empty list.
The service checks connection generation and work-thread ownership again after
reading. The panel refreshes while open and drops old reads on navigation or mutation.

Users can associate an HTTP(S) link or remove an association without changing the
underlying resource. The application-owned type decodex.link uses a SHA-256 identity
of the normalized URL and a title/URL payload; this is a Decodex convention, not an
upstream type. Duplicate adds keep the original stored title. Invalid schemes and
embedded credentials are rejected before dispatch. Mutations use the existing
command receipt path and never automatically retry. A separate complete native
read verifies the requested state after a mutation, including an uncertain reply.
The UI does not infer success from a failed read or partial list.

Protocol 2.31 carries the new query and commands. Native payloads remain opaque;
large or credential-bearing display payloads are explicitly omitted. The display
has a complete-list capacity limit rather than silently presenting partial data.
Controlled transport, projection, navigation and readback tests cover the local
consumers. An isolated acceptance run with installed codex-cli 0.155.0-alpha.9.2
also used two WebSocket clients against one native process: observer pagination,
exactly two create/two delete notifications, repeated add/remove, preserved original
metadata and final empty readback all passed. No model turn was submitted. A newly
started but unpersisted empty thread correctly returns not-found; the successful
run resumed a persisted fixture and used the returned native identity. This is
native API acceptance, not a claim of full desktop/model end-to-end coverage.

## MCP client admission follow-up

The earlier MCP elicitation change updated service projection and GPUI controls but
missed AgentClient's request-method admission check. As a result, a valid MCP form
returned by the service was rejected before it reached the UI. The client now
admits mcpServer/elicitation/request while retaining exact event identity and
unknown-method rejection. A real local WebSocket exchange regression failed on the
old admission check and passes after the fix. Directly injecting a form into GPUI
was insufficient evidence for this transport boundary.

## Plugin and MCP state audit in progress

At the fixed upstream snapshot, thread disabledPluginIds remains a saved selection
that does not filter plugin capabilities. The final thread.rs contract still says
this explicitly. Commits b5544d5732f4 and c62d191c4c8c define replacement semantics:
omitted/null preserves the selection; an empty array clears it. Existing Decodex
turn requests omit the field and preserve native state. A future control must not
claim that saving this selection disables tool execution.

Commits 343074d4207d, 8f31b64c7f9e, d996b4f02a20 and 7a6f469dcff1 separate runtime
connection state, tool discovery failure, authentication and advertised server
capabilities. Empty tools with a discovery error is not a confirmed empty catalog;
a cached catalog is not proof of a live authenticated connection. Native status-only
OAuth discovery now changes OAuth to notLoggedIn after an authentication failure.
The client must preserve those facts rather than infer readiness from tool count.

Commit c4c51c56e4f3 makes plugin discovery sensitive to repository configuration.
Use plugin/installed with the exact task thread cwd, not a home-only catalog.
Marketplace load errors must remain visible even when some plugins are listed.
The native thread/read metadata carries cwd; confirm the returned thread identity
before using it. Decodex's existing process launch retains the shared native home
and does not replace its plugin state with an account-specific copy.

The new native client adapters preserve complete paginated MCP state for an exact
thread and repository-scoped installed plugin responses including load errors.
The task panel displays separate runtime, authentication and plugin policy states.
Reconciliation receipt flags describe changed bundle categories, not runtime readiness; native
MCP reload and refreshed status need separate treatment. OAuth credential refresh,
issuer binding and provider-policy enforcement stay with native Codex.

The in-progress integration surface now has separate read-only status refresh and
explicit shared synchronization. The latter calls plugin/reconcile once, validates
its receipt, and calls config/mcpServer/reload even when reconciliation reports a
partial bundle failure. Partial failure remains an explicit outcome. A native reload
acknowledgement is not a connection-readiness claim. The UI reads current status
after the operation and does not automatically replay an uncertain write.

Source review of plugins/reconcile.rs confirms that native Codex owns hook trust
and lifecycle refresh. mcp_refresh.rs plans per-thread configuration using each
loaded thread's original config layers and cwd, then applies MCP inputs; unrelated
model settings are not copied from the global config. The UI describes this shared
scope. An isolated codex-cli 0.155.0-alpha.9.2 run verified repository plugin catalog,
thread-scoped MCP status, reconciliation receipt, null-parameter native reload and
second-client status readback without a model turn or sign-in.


Native MCP sign-in uses mcpServer/oauth/login with the exact task thread and server.
Codex owns registration, PKCE, callback validation and credential storage. Decodex
keeps the authorization URL in a bounded, diagnostic-redacted in-memory response;
it never writes the URL to command receipts or SQLite. Opening the link requires
an explicit click and does not complete sign-in. A reopened window can recover the
same pending native flow without another login request. Polling, timeout and
connection loss never replay authentication automatically.

The native completion notification has threadId and name but no attempt ID.
Decodex therefore reports a native server/thread observation, not proof of a unique
caller attempt or a ready tool connection. It checks the event receiver's original
process generation and removes the link on completion, disconnection or expiry.
A local wait timeout does not cancel native work. Status refresh verifies runtime
state separately.

An isolated local OAuth/MCP fixture with codex-cli 0.155.0-alpha.9.2 verified native
PKCE callback, exactly one token exchange, completion on two connected clients
with matching server/thread identity, reload and authenticated MCP status. The
fixture uses a temporary native home and file credential store, no real account,
and no model turn. Local AgentClient WebSocket tests also verify exact login intent
transport and rejection of another session's response.


## Account and task usage boundary review

At fixed upstream 595cc91e8, rawResponse/completed is explicitly internal-only.
thread_lifecycle.rs drops raw response items and completion events unless the
thread was started with experimentalRawEvents. The resume API has no equivalent
field. Thus a fixture that injects this event cannot prove delivery for existing
or restarted Decodex tasks. Decodex does not enable this internal event stream or
maintain a second ledger from its amount string. Commits 2c4a95736bea and
e017e93aceaf preserve response precision for native internal consumers; they do
not make raw metadata a default durable client history contract.

The supported account/usage/read endpoint accepts threadId and returns native
backend estimates. Decodex now reads these estimates on explicit request from the
task panel. Integer millionths preserve credits and optional USD exactly; missing
USD or token counts stay unknown. Cached input is a subtotal and is not added to
input. Each result shows the authenticated local account and observation time.
The service checks process generation, account credential revision and exact
work/thread binding before and after the read. Changed sources discard the result.
No estimates are added to account quota or copied into a durable accounting ledger;
reopening and refreshing read the native authority again.

Native account_processor.rs bounds the backend thread-usage request at 60 seconds.
The service uses 65 seconds and its local client uses 75 seconds. Native 403/404
become absent estimates; transport errors, unavailable source and unsupported
methods remain separate outcomes. Backend results with the wrong thread are
rejected. Native usage supports externally managed ChatGPT authentication, matching
Decodex's retained process owner.

An isolated local backend and codex-cli 0.155.0-alpha.9.2 verified exact-thread
estimates, nullable USD/counts versus zero, 403 absence, wrong-thread rejection and
a second client's reads after an account switch. No real account or model turn
was used. Local transport tests preserve integer values above 2^53. Runtime tests
change account, credential revision, process and thread between request and reply;
each changed source is rejected. A rendered GPUI test checks explicit opening and
clearing on task navigation.

Workspace routing from a4354e2d27fd belongs to model request discovery. Final native
account rate-limit and usage reads still construct BackendClient from the configured
chatgpt_base_url; BackendClient::from_auth does not apply WorkspaceRouting. That
commit alone does not justify sending Decodex account-observation requests to the
model backend origin. Broader custom-backend/FedRAMP applicability remains under
review. TUI Analytics independently binds both account and user and selects reports
from the server's accounts/check plan, not token plan claims; its remaining report
capabilities still need disposition.


### Server-advertised experimental settings (source 792)

Commit `048a936a23b88c8653f4820e68f987de10e3c583` and the fixed cutoff
use native feature discovery and configuration writes for server-advertised
experimental controls. The client must retain uncertain selections, read back
configured values after a write, report overrides, and keep active task settings
separate from defaults for new tasks. Dedicated permission and native voice
controls keep their own behavior.

Decodex currently shows only a read-only memory flag from Chief capabilities.
It has no complete feature settings editor or source-bound write/readback owner.
The installed protocol supports these operations, but the existing app-link
settings writer does not implement this feature. This gap remains open.


### Voice helper negotiation (source 793)

The upstream private helper owns bounded WebRTC negotiation and reports transport
readiness after its ordered event channel opens. At the fixed cutoff, its README
still lists receive/decode and TUI integration as future stages. It does not yet
replace the existing Decodex bidirectional media host.

The WebKit host now reports connected only after both the peer and its local
ordered event channel are ready. It closes remote-created channels and retires
the call on channel loss. Production-script tests cover callback order, deadlines,
cleanup and stale callbacks. A native WebKit synthetic-audio offer/mute/stop test
also passes. Full remote negotiation, physical devices and voice UI acceptance
remain open independently.


### Native sandbox settings (sources 796–798)

The installed Codex 0.155.0-alpha.9.2 enforces the macOS user-config opt-in for
symlinked writable roots beneath CODEX_HOME. Isolated native execution refused a
write with the setting disabled and wrote the expected file with it enabled.
Decodex retains its fixed shared-home binding and lets native Codex resolve this
policy; separate credential and executable checks stay in place.

A native execution fixture also verified unified_exec_tty=false: explicit TTY
execution was refused, while false and omitted TTY settings ran successfully.
These are native model-tool settings, not new Decodex execution owners. Neither
probe used user credentials or a real model provider.


### Model-owned Guardian policy (source 799)

Native Guardian binds cached assessments to model policy, thread settings,
environments and authorization context. At the cutoff, Code Mode wrappers have
no separate approval scope; their nested tools follow the resolved policy.
Managed review requirements remain authoritative.

The installed public model catalog does not expose this private policy. Decodex
must continue to show observed native reviews and submit only exact user-approved
denials, without caching risk scores or guessing policy from model names. The
complete native policy and rendered approval matrix remains unverified.


### Native provider and restore metadata (sources 898–900)

Chief model details now show the provider ID from an exact native thread read.
Missing metadata does not inherit a local provider label or URL. Source checks,
rendered socket tests and installed-native observations pass (current protocol 2.56).
Ordinary resume now sends only thread identity and excludes turns; it no longer
reapplies saved model, directory, instructions or service tier. Contract and
installed-native tests pass. The ordinary typed start/resume response now retains
and validates the current top-level provider ID. A three-process native fixture
changes the saved model and directory through another client, then verifies that
cold resume reports those values, rejects the old directory, and sends no model
request during resume. This is adapter evidence, not desktop acceptance.

Ordinary start/resume now retains source-bound observations in SQLite (schema 38).
The read-only conversation projection and inspector show the last native model,
provider, directory and effort (protocol 2.56). The source binds the exact session,
thread, ready process and account revision; late responses cannot replace newer
facts. A replacement process needs confirmed prior death. Original request and
next-message execution settings remain separate. Conversation list/detail reads
use one SQLite snapshot for lifecycle, source binding and observation data.

Native directory changes still require explicit local reconciliation before
execution. The existing mismatch gate remains. A rendered fixture checks draft
and execution preservation; installed adapter and database restart tests remain
separate evidence, not full native desktop acceptance. Live settings publications
and explicit directory reconciliation still need integration. No active local
fork or rollout-file restoration path was found, so the CLI fork and rollout-parser
changes need no equivalent route.


### Resume and directory event stacks (source 901)

Upstream defers TUI resume-picker and directory transitions until event dispatch
returns. It rechecks the source session, directory and idle state. Its directory
command forks saved history or starts a new thread after destination configuration
and permission checks. Decodex has no embedded TUI or local fork route, so this
stack fix requires no local counterpart.

Ordinary continuation now sends the selected thread's persisted native directory,
when available, instead of the application launch default. An invalid observed
path does not fall back to a different workspace. Explicit message settings stay
unchanged and runtime still validates the path and exact resumed thread. For older tasks without native observations, the protocol now supplies the original
request directory from SQLite. Continuation uses that saved path and never falls
back to the current application default; absent saved paths prevent submission.
Native cwd changes rejected by the current resume gate and full context recovery
remain incomplete.


### Guardian classifier transport (sources 902–903)

Codex keeps WebSocket establishment outside classification dispatch. When no
healthy idle socket exists, it uses HTTP with the same concurrency and sampling
retry limits. Native cancellation covers header waits and response draining.
The cutoff routes classifier requests through `/responses` with native Guardian
headers. Decodex has no classifier transport implementation; it must reuse Codex
and keep explicit approval receipts separate from classification outcomes.

Local Guardian controller tests pass. The installed binary is now
`0.155.0-alpha.16`; six relevant experimental start/resume and Guardian schemas
match the prior `0.155.0-alpha.9.2` bundle. This does not verify native stalled
handshake recovery, Guardian V2 cold reverts or full desktop acceptance. Those
gaps remain. Source 903 changes only two TUI test fixture initializers.
