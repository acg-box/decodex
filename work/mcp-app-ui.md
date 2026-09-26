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
