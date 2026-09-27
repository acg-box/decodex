# Activity and timeline reconciliation

Classification: core projection of existing native activity and history. Keep
native Codex as the execution and authentication owner. This recovery does not
start login, replay a tool call or add an authentication flow.

## Restore the native reconnect label

The complete inherited activity diff revealed a missing authentication helper
and two missing regression cases. Restore the helper and both authentication
projection tests. A completed, failed MCP tool item with a nonblank
`result._meta["mcp/www_authenticate"]` string or a nonempty array of such strings
is labeled "Sign-in required". Running calls, successful calls and malformed or
empty metadata retain their existing labels. Raw challenges are not projected.
The status remains failed, and the detail contains only the existing bounded
server/tool label.

Fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582` emits a string for
local authentication expiry and an array for an HTTP challenge in
`rmcp-client/src/rmcp_client.rs`. Its `core/tests/suite/mcp_oauth_refresh_tests.rs`
checks a failed completed tool with the same metadata and no replay of that call.
The source was read, not executed here. The upstream app-server MCP result type
and the installed 0.158.0-alpha.2.1 experimental history schema both retain
`_meta`. This is schema/source evidence, not a live OAuth qualification.

Before restoration, both local regressions fail with "Using tool" instead of
"Sign-in required". After restoration, string and array forms produce the label;
serialized activity and history do not contain the private challenge fixture.
Restore the inherited web-action privacy test with the current equivalent labels
"Opening web page" and "Finding text on page". URL and search-pattern parameters
remain absent from activity details.

## Complete runtime file mapping

`chief/activity.rs` retains the inherited bounded details, subagent identity
checks, duration and completion status. Reordering the tool-result match arm and
using a JSON pointer for web action type do not change accepted data. Keep the
current search-exit-one distinction: it reports exited and retains the exit code,
without claiming search success or failure.

`chief/timeline.rs` moves the old `read_full` body into its caller. The page-size
sequence, timeout, source checks, native error handling, enrichment and result
bounds remain. Summary fallback is still first-page-only, after a failed full
read and before/after matching source checks. It never appends a fallback to a
cursor page or replaces an available page.

The added App UI flag recognizes native MCP resource metadata with a nonempty
`ui://` URI. Other item kinds or HTTP URLs do not acquire an App entry. This is
separate from App resource execution and its acceptance. Reasoning still skips
ordinary activity completion projection. Restore the inherited authentication
history regression without changing raw metadata visibility.

## Complete desktop activity file mapping

`chief_activity.rs` retains the inherited message keys, history marks, navigation,
follow pause and jump controls. Exposing mark position supports the current
message-based scroll anchor. Upward scrolling delegates prefetch to the existing
history owner, which checks selected work, loading state and cursor availability.
It does not create a second history reader.

After prepend, the workspace restores the visible message position or uses the
change in scroll extent. The anchor is discarded on a task change. Follow-latest
now sets the retained follow state; the workspace approaches the current bottom
on render frames. Existing final-position assertions remain, with enough draws
for that animation. Keep the new rendered prepend-position regression. This
mapping does not close the larger workspace or timeline-render files.

Validation passes six runtime activity tests, 39 runtime timeline tests and 13
rendered desktop activity tests. Strict all-feature, all-target runtime and GPUI
Clippy also passes.

Verify all three original snapshot hashes and close only these three inherited
file rows. Local projection and rendered navigation tests do not establish signed
desktop acceptance, successful provider authentication or full R03/R06/R09 scope.
Automations remain paused.
