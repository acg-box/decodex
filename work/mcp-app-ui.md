# Interactive MCP App UI

Classification: optional product capability. Status: implementation in progress;
R05 is open. A native transport adapter alone does not deliver an interactive widget.

## Native contract

Fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582` exposes
`mcpAppUi.resourceUri` and preferred display mode in MCP tool-call events and history.
Installed Codex 0.158.0-alpha.2 supports `mcpServer/resource/read` and
`mcpServer/tool/call`; an isolated synthetic MCP server returned HTML, CSP metadata,
structured tool output and result metadata without a model turn.

Use the existing app-server connection. Resource reads require the exact loaded
thread, server, originating call and ui:// URI. Preserve resource metadata for the
view's sandbox policy. For codex_apps, require the response to confirm the originating
call, rather than accepting a global discovery fallback. The service must also check
work, account and process identity before and after reads. The installed schema has
an additional explicit target field; this is not part of the fixed upstream contract.

## Exact native item selection

`mcp_app_for_item` reads the exact thread/turn/item through native paginated history.
It rejects duplicate identities and non-MCP items. Resource metadata uses the current
mcpAppUi field first, then legacy mcpAppResourceUri and appContext.resourceUri fields.
Malformed preferred metadata is not silently replaced by a legacy URI. It retains
the complete native item, resources and a live history/settings/connection guard.
A revert during resource loading invalidates the result. This native guard does not
replace service account/process ownership checks.

Four adapter tests cover scope confirmation, unavailable resource handling without
retry, exact native history with a concurrent revert, and ambiguous/legacy item
selection. The strict adapter Clippy check also passes.

## Service document reads

Protocol 2.88 adds `GetChiefAppUi` and `ChiefClient::app_ui`. The request identifies
work, thread, turn and item; it does not accept a resource URI or server from the UI.
The service checks its current account, process generation, credential revision and
history before and after the native read. It returns the original item and resource
contents as a JSON document in 32 KiB chunks, up to 6 MiB. A SHA-256 fingerprint binds
the complete bytes and source. Every continuation requires the same fingerprint;
a changed resource fails instead of mixing content from separate reads.

The desktop client checks the exact echoed request, fingerprint and byte bounds.
The selected protocol/runtime run passed 165 tests, including real local-wire
malformed responses and account/process/history changes during widget reads.
Strict protocol/runtime Clippy and 15 architecture tests passed. These checks do not
establish an interactive view or installed application acceptance.

## Native view document preparation

`McpAppDocument` selects one exact ui:// HTML resource from the service document.
It supports UTF-8 text or base64 UTF-8 content and nullable legacy URI fields. Duplicate
resources, ambiguous text/blob content and unsupported MIME types are rejected.
Declared CSP domains are validated as explicit HTTPS origins (WSS for connections),
including wildcard subdomains; policy injection, credentials, file URLs and path/query
forms are rejected. An initial CSP meta element restricts network, frames, objects and
forms. The consumer must still enforce the opaque sandbox, navigation and browser
permission boundaries. Four focused Swift document tests passed.

The lifecycle reference is the [MCP Apps specification](https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/2026-01-26/apps.mdx).
Document preparation is not a rendered widget or complete sandbox acceptance.

## WebKit lifecycle implementation

`McpAppView` loads the prepared HTML in an opaque-origin iframe with script permission
only, inside a nonpersistent WKWebView. The parent relay accepts messages only from
that iframe, and the native handler accepts only main-frame relay messages. It handles
2026-01-26 initialization, waits for the initialized notification, then sends the
original tool input and result. Ping is supported. Tool and resource callbacks are
not advertised or forwarded yet. The view denies media capture, file panels, new
windows, JavaScript dialogs and external navigation. It removes handlers on close.

Five Swift tests pass, including a real WebKit fixture that receives the tool result,
cannot access the host DOM, and cannot call the native handler directly from the
child frame. This is browser lifecycle evidence, not complete desktop integration.
External nested frames, display-mode changes, graceful resource teardown, signed lifecycle acceptance and explicitly mediated interactive callbacks remain open.

## Native window ABI

`McpAppHost` exposes versioned create, command, poll and destroy functions in the
existing Swift library. A host accepts one document and attaches one child panel to
the desktop window. It refuses replacement and reuse after closure. Closing removes
the child window, closes WebKit and releases the retained event buffer on destruction.
The ABI carries copied JSON and does not own task state or tool permissions.

Six focused Swift tests pass, including the real native ABI create/load/close sequence
and rejection of a second document. GPUI connects this ABI through the timeline action described below.

## Desktop entry and document collection

Native timeline items carry an explicit app_ui flag derived from MCP resource metadata.
Only those items show Open app. The action creates a native host, collects the document
through the source-bound service query and opens the child window after checking the
selected task, timeline epoch, request serial and account binding again. A different
account or a cleared timeline drops the host and outstanding request. Native closed
and unavailable events are consumed during desktop rendering.

The collector checks the account and total length across all chunks; the typed client
checks each echoed request and fingerprint. It parses JSON only after the complete
length arrives. Tests cover actual local-wire collection and account refusal, native
metadata projection and rendered action visibility. The selected desktop/runtime/
protocol run passed 247 tests. Clippy passed with the repository's existing unused-import
and dead-code allowances. The first run had a malformed new test fixture (missing
agent-message text); the corrected full run passed with no leaky warning.

This does not establish signed desktop visual acceptance or tool callback authority.

## Display source lifetime

Each document chunk also returns an opaque source fingerprint that covers its task,
thread, account, process generation, credential revision and history revision. The
collector requires one source fingerprint across all chunks. `GetChiefAppUiSource`
compares that identity with the current service owner and checks the native connection
guard. It does not read HTML, call MCP tools or start model work.

After opening the view, the desktop checks immediately and then once per second.
A changed source, failed query or disconnected service drops the native host and
shows a refresh notice. Closing the native window or changing the selected timeline
cancels the monitor. The selected protocol/runtime/desktop run passed 173 tests, with one leaky-handle
warning on an unchanged account-recovery transport test. That test passed in isolation
without the warning. Clippy passed with the existing desktop allowances. Tests include
the real lightweight local query and desktop host closure after a stale response.

This is display lifetime control; each future effectful callback
still requires a fresh authority check at dispatch.

## Tool transport boundary

`call_mcp_app_tool` performs one native mcpServer/tool/call request and preserves
content, structuredContent and result metadata. It does not forward widget-supplied
transport metadata. Malformed responses and lost replies remain errors whose effect
status is unknown; the adapter never retries. Five native adapter tests pass, including
lost and malformed responses. Strict adapter Clippy passes.

The adapter is not exposed to the view yet. Before dispatch, the service must confirm
the target tool belongs to the originating app/server, obtain explicit user authority,
and durably reserve the exact arguments. Direct native calls bypass the model-tool
approval path. Existing configuration receipts arbitrate config-file writes and must
not be reused with misleading tool-result states. The existing task event store can
retain a distinct tool-attempt/result lifecycle without a second database. Hosted app
tool ownership requires authoritative catalog evidence, not a tool-name guess.

## Durable tool attempts

Schema 45 marks the App UI call persistence contract. `chief_app_ui_calls` uses the
existing immutable inbox events for small routing records and chief_request_payloads
for complete arguments and results. It creates no second database or execution owner.
Reservation checks the live task/thread/process/account and atomically rejects an
existing attempt, reused review token or unresolved call for that work item. Only the
caller that receives a new reservation ID may dispatch.

A result is stored once as completed, unknown or positively unsent. Completed means a
native response was recorded, not that the tool reported success. Unknown remains
unknown after restart. An explicit acknowledgment permits later, separately confirmed
calls but does not change the old outcome or permit replay of its review. A pending
lookup makes unresolved calls discoverable after a view restart.

The complete database suite passed 147 tests. The strengthened focused tests also
passed after adding pending lookup, unknown-result reopen and review-token replay
checks. They preserve arguments/results larger than 64 KiB and reject another account
or work owner. Strict database Clippy passed. Runtime confirmation and callback dispatch
are still pending; these persistence APIs are not exposed to the widget.

## Callback catalog and connection identity

`review_mcp_app_tool` reads the exact originating item and the loaded thread's MCP
catalog. It requires a connected server, a unique raw tool name and app-visible tool
metadata. Model-only tools and legacy widgetAccessible=false tools are excluded.
For codex_apps, it cross-checks the enabled tool in `app/read` for the exact connector;
metadata summaries alone never establish runtime readiness. It compares the effective
link with the originating appContext, using the fixed upstream account rule: explicit
link_id arguments when required, otherwise descriptor _meta.link_id.

This follows the fixed source in core/src/mcp_tool_call/account.rs and the public
AppsReadParams/AppsReadResponse schemas. Raw names are compared exactly; normalized
model namespaces are not used for native dispatch. The returned review retains the
original item, live descriptor and history/settings guard. Tool dispatch now passes
that guard to the native transport writer, which can refuse a stale or foreign review
before writing. Runtime still must reserve the reviewed invocation before calling it.

Eight adapter tests cover catalog ownership, visibility, connection identity, exact
read-only review, response preservation and foreign-guard refusal. Strict adapter
Clippy passed. Hosted installed-native integration acceptance is still required.

## Runtime confirmation and dispatch

`ReviewChiefAppUiCall` returns an exact invocation echo, native server/title, review
token and any unresolved operation. The complete local confirmation request is bounded
to 64 KiB. The token includes the source, original item, live descriptor, exact arguments
and host operation identity. A fresh confirmation is a separate `ConfirmAppUiTool`
action. It re-reads evidence, rejects a changed token, reserves once, rechecks source
and sends with the captured native transport guard.

A recorded response is saved before acknowledgment. Positive pre-send source changes
are saved as unsent. Lost/malformed native replies are unknown. Existing operation IDs
are rejected before any native review or dispatch; callers must use durable readback.
Review reads are bounded to 30 seconds; the command transport allows review plus the
60-second native call. The browser still has no direct authority to execute this action.

The selected protocol/runtime run passed 168 tests. A strengthened real-store/native-wire
test also passed for successful calls, lost replies and source change after reservation.
It rejects changed arguments with an old token, proves the dispatch count is at most
one, and reads exact results after database reopen. Strict protocol/runtime Clippy
passed. Desktop confirmation and bounded receipt transfer are still pending.

## Durable receipt transfer and recovery

The local receipt query reads the saved invocation and outcome without a live native
connection. It transfers bounded 32 KiB chunks and binds each continuation to the
complete document hash. A changed result or acknowledgment invalidates continuation;
readback does not dispatch the tool again.

An unfinished reservation becomes unknown only after positive evidence that its
original process died. A missing process record or uncertain death does not settle the
call. Explicit acknowledgment applies only to the exact unknown reservation and does
not change its outcome to success or replay it.

The current-source selected protocol, runtime, database and desktop run passed all
179 tests. The native-wire fixture preserves a 70,000-character result through store
reopen and chunked receipt transfer. Tests also reject mixed receipt content and
invalid wire bounds, and distinguish live, death-unknown and positively dead process
states. Strict protocol/runtime/database Clippy passed. Desktop confirmation, receipt
presentation and browser response delivery remain incomplete.

## Desktop callback integration in progress

The isolated WebKit view can emit one pending tools/call request with a native UUID.
Repeated browser IDs, including changed arguments, cannot replace that pending
operation. A distinct request receives a busy error. Native responses must match the
host operation before the view returns the original browser RPC ID. Tool events cannot
be dropped by a full queue of status pings. The host accepts larger result commands
separately from the existing document-load limit.

The desktop controller binds each callback to its displayed source, queries the
service review, shows the exact server/tool/arguments, and submits only from its local
confirmation action. After submission it reads the durable receipt even if the local
command reply is lost. It never resubmits from that result path.

This integration remains disabled in the document load command. It must stay disabled
until unresolved receipt read/acknowledgment and cold recovery are accessible in the
consumer, followed by complete native-wire and rendered confirmation tests. Successful
result forwarding alone is not interactive delivery. The real WebKit suite passed
seven tests, including duplicate and conflicting browser requests and exact reply
routing. Signed desktop acceptance and R05 delivery remain open.

## Saved outcome controls

The callback consumer now retains the submitted receipt identity when the view closes
or its source becomes unavailable. The saved-outcome action only reads the local
journal. Unknown outcomes have a separate explicit acknowledgment action, bound to the
saved reservation. Its reply is followed by receipt readback; it does not assume success
from a missing reply. An earlier tool review is discarded after acknowledgment, so a
later call requires a new request and review.

The view receives saved completed results or an explicit unsent/unknown error. Reserved
and unavailable outcomes remain unresolved. Readback validates the saved document's
work and operation IDs as well as the local chunk envelope. A local-wire fixture covers
query-only two-chunk reads, rejection of a foreign saved owner, and retention of an
unacknowledged unknown outcome when the view closes.

Cold discovery still requires a separate work-owned pending-operation read that does
not depend on a new live widget review. The current view controls do not complete that
requirement. Keep tools/call capability disabled until cold discovery and rendered
confirmation/recovery acceptance are complete.

## Cold pending-call discovery

A work-owned local query now discovers the unresolved operation from the existing
journal without a live Chief or native process. The response separates a successful
empty read from unavailable storage. The typed client checks the echoed work owner.

The conversation panel includes a recovery action independent of widget visibility.
It discovers the operation, collects its saved receipt, and exposes the same read and
acknowledgment controls used after a live call. Confirmation and recovery controls are
rendered once at the task level. A task switch rejects a late recovery result.

The current selected run passed 186 tests. The native-wire fixture verifies pending
lookup after database reopen for lost, completed and unsent calls. Desktop tests verify
cold discovery without a widget, unavailable versus empty results, foreign-owner
rejection, and rendered recovery/acknowledgment controls. The initial repeat found an
unrelated output-stream OS thread in the deterministic GPUI fixture; the fixture now
clears the surface profile after the recovery task captures its local client. Runtime
and protocol strict Clippy passed; desktop Clippy passed with its existing allowances.

The tools/call capability remains disabled pending complete confirmation/recovery,
exact lost-reply command behavior and signed native consumer acceptance. This local
recovery path does not itself establish full App UI delivery.

## Confirmed callback path enabled for desktop acceptance

The desktop load command now enables serverTools in the native view. The browser
request path only prepares a service-backed confirmation. Allow consumes that review
once; repeated clicks do not submit another command. Cancel returns a browser error.
After a lost command reply, the consumer reads the saved outcome. The same rule applies
to the separate unknown-outcome acknowledgment. Closing the widget keeps the submitted
call's receipt read alive.

Twenty selected desktop test instances passed, including real local-wire review-only,
lost execution reply, lost acknowledgment reply, repeated clicks and source/owner
checks. Seven native Swift tests passed; the real WebKit bridge received a text result
of 8 MiB minus 1 KiB in full, with the original browser RPC ID. This qualifies component
behavior, not signed whole-app or installed-native acceptance. Earlier notes that the
load command omits toolCallsEnabled are superseded by this section.

Signed desktop interaction, installed native integration and the remaining consumer
obligations below are still required before R05 closure and normal PR delivery.

## Bounded resource teardown

The native view sends ui/resource-teardown before closing an initialized connection,
as required by the [MCP Apps lifecycle specification](https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/2026-01-26/apps.mdx).
It stops admitting new widget requests immediately. An exact teardown response releases
WebKit handlers and delegates; no response reaches the same cleanup after 500 ms. A
terminated WebKit process is released immediately because it cannot respond. The view
retains itself only through this bounded cleanup, even if its native panel is destroyed.

Eight native tests passed. Real WebKit tests cover exact response matching, an unrelated
response, a silent widget, repeated close and rejected tools/call during teardown.
The prior signed artifact still represents a6688590b; it does not contain this change.
A final signed build must include this lifecycle update before acceptance is complete.

## Native display modes and dimensions

The fixed upstream protocol contains preferredModelDisplayMode. The native host now
honors fullscreen preference when the widget supports it. Fullscreen fills the owning
window's area in the child panel; it does not create a separate macOS Space. Returning
to inline restores the previous panel frame. The panel can also be resized normally.

Initialization reports actual view dimensions and the intersection of supported host
and declared widget modes. ui/request-display-mode returns the actual mode, including
when a request is declined. Host-context notifications carry mode and dimensions after
changes. This follows the [MCP Apps display-mode requirements](https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/2026-01-26/apps.mdx).

Nine native tests passed, including actual NSPanel geometry, a widget that excludes
fullscreen, restoration to inline, declined picture-in-picture, and notification of
an 800 by 600 content resize. These component tests do not replace final signed visual
acceptance. Earlier descriptions of a fixed 720 by 480-only host are superseded.

## Installed-native history and callback qualification

An isolated local Responses fixture and MCP server passed against installed
codex-cli 0.158.0-alpha.2. The model invoked the MCP counter through native
functions.exec. Native item/completed emitted one MCP item with mcpAppUi,
mcpAppResourceUri, exact arguments, structured result and metadata. The paginated
thread/items/list response returned that exact item under the same turn identity.
Resource read returned its matching HTML. A direct native callback returned the new
counter value and retained metadata without another model request.

The two model requests both reached the local synthetic backend. No real account or
external model was used. The native process and HTTP fixture were stopped. Reproducible
probe and report are retained in the task evidence directory as
native-app-ui-turn-20260926.py and native-app-ui-turn-20260926.json.

Initial probes assumed a top-level function-call tool list. The installed binary uses
additional_tools with a functions.exec namespace; those attempts did not produce an
MCP history item and are not positive evidence. The final probe asserts exact item
identity and contents, callback result metadata, completed status and unchanged model
request count after the callback. Hosted codex_apps connection qualification and the
combined signed desktop/service/native interaction remain separate acceptance work.

## Desktop confirmation and recovery visual review

The existing visual-capture binary now accepts DECODEX_VISUAL_APP_UI=confirmation or
unknown for synthetic layout review. It renders the actual Chief confirmation and
saved-outcome controls with no service profile. It rejects combination with a live
service-root capture to prevent synthetic layout from being recorded as service
acceptance. No new product runtime or separate renderer was introduced.

Both scenes were captured and inspected at 1248 by 840 logical pixels. The review
found duplicate unknown-outcome text; the panel now shows that status once while
retaining distinct connection notices. Exact tool arguments and action labels were
visible without overlap. Evidence images are target/visual-tests/r05-confirmation.png
and target/visual-tests/r05-unknown.png in the task worktree. These are component
layout evidence, not a signed whole-app service/native interaction or user approval.

## Remaining consumer obligations

- Complete signed desktop visual acceptance of the source-bound document action.
- Host untrusted HTML in a separate restricted view within the existing Swift module.
  Do not reuse the trusted voice document or its microphone grants.
- Implement MCP Apps initialization, input/result notifications and teardown. Advertise
  only capabilities with implemented handlers. Apply declared CSP and permissions.
- Bind every callback to its live view and native source. Native direct tool calls
  bypass the model tool approval path, so effectful widget calls need explicit local
  confirmation and durable uncertain-outcome handling before exposure.
- Qualify a rendered interactive fixture, source changes, rejected permissions,
  uncertain replies and cold recovery before normal delivery and R05 closure.

The transport adapter does not expose tool execution, create another MCP connection,
store credentials or grant a widget authority over other threads.
