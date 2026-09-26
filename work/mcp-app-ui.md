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

## Remaining consumer obligations

- Resolve the resource from the exact native tool item and preserve source identity.
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
