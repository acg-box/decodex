# Reconcile the shared native client and settings guards

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Read the complete inherited/current diffs for the four files below and inspect
their current callers. This batch changes no production or test source.

## Shared transport

The complete 653-line diff for
`crates/decodex-codex/src/app_server_client.rs` retains all inherited module
registrations and function names. The following table accounts for its changes.

| Change | Current owner and effect |
| --- | --- |
| Module and export order | Existing owners remain. Additive modules expose App UI, dispatch refusal, goals, hooks, defaults, prompt editing, temporary structured calls and thread model selection. Their registration is not new feature acceptance. |
| Closed local refusal variants | Stale request guards, oversized requests and full outbound queues have distinct errors. They establish that this request was not sent; inbound overflow and I/O failure remain uncertain. |
| Request preflight | Count the complete JSON-RPC envelope, escaped content and maximum positive request ID before a related side effect. Chief submission and prompt-upload preparation use this owner. |
| Connection identity | Both transport constructors create an opaque identity; clones retain it. File approvals, voice settings and recap source checks use it to distinguish reconnects. It is not an authentication credential. |
| Voice transcript classification | Only user/assistant realtime delta or completed text counts as visible input. Settings and recap consumers invalidate captured sources without requiring a task turn. |
| Settings readers | Permission and plugin observation methods move without behavior loss. Configured plugin/model and idle-model reads use ServerRequests and reject closed transports. Configured facts do not describe an active step's effective settings. |
| Combined guard | Add a settings constraint while retaining the existing question/history revision and connection identity. Reject foreign or stale guards. |
| Permission hydration | Move the original start/resume response checks into PermissionHydration::observe. Start still requires the captured global hydration revision; resume requires an exact thread and live guard. |
| Outbound dispatch | Reject a new request at the pending limit without disturbing peers. Both sinks reject oversized frames before forwarding. A full framed channel is also a local refusal. Other write failures close the connection with pending outcomes uncertain. |
| Tests | Retain inherited tests and helpers; add exact frame/envelope, peer preservation, queue refusal and inbound-uncertainty cases. |

The stdio writer checks serialized size before write_all. The framed sink checks
size before try_send. Only those known-unsent errors receive local-refusal
classification. Neither path authorizes replay after an uncertain I/O result.
Raw requests remain single-attempt; the separate resume helper retains its exact
closing-refusal retry contract.

## Settings observations and revisions

`permission_observations.rs` has one changed statement: retain the invalid guard
as a revision anchor when a turn starts. ServerRequests invalidates that guard
before calling start_turn. Idle reads still reject an active turn or invalid
guard. Exact matching completion creates the new guard; unrelated completion or
malformed settings cannot revive the old authority. This generic owner serves
permissions, plugins and model settings with the existing row and byte bounds.

`settings_guard.rs` adds a shared monotonic revision allocator and a revision
accessor. Values do not regain an old identity after A-to-B-to-A changes or after
all guards for a thread are dropped. Dead weak entries remain bounded. Saturated
revisions and clear operations invalidate guards. Added combined-guard tests
retain question revision, task isolation and connection ownership.

## Shared thread model projection

`thread_model_settings.rs` adds NativeThreadModelSettings::from_read_response and
uses it from the existing asynchronous reader. The existing projection rules are
unchanged: exact thread ID, nullable versus absent metadata, bounded native
spellings and no invented provider fallback. The wrapper validates its expected
thread input. The ordinary process reader now reuses this same parser for its
exact thread/read response, without resume or turn/start. Process and account
authorization remain the callers' responsibility.

## Evidence and limits

All 141 selected app_server_client tests pass; seven installed-native tests remain
ignored in this run and are not counted as qualification. The passing suite
includes settings revision identity, combined guards, exact idle turn completion,
hydration invalidation, nullable model metadata, queued settings rejection, frame
size, outbound queue refusal and bounded closing retry.

Original snapshot hashes match all four register rows. Close only these complete
file dispositions. The runtime shared owners, installed shutdown/unload races,
version-specific native limitations and signed desktop acceptance remain separate
requirements. The shared transport and source guards are core even when a caller's
settings surface is optional. Automations remain paused.
