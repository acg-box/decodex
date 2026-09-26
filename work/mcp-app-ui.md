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

## Remaining consumer obligations

- Connect the desktop view to the source-bound service document query.
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
