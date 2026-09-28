> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Restore complete integration status

The integration projection retained the top-level MCP capability names but dropped
extension names. The desktop also omitted the capability line and connector IDs.
A restored regression fails before the fix: `extensions/openai/settings` is absent
although the native status contains it.

Restore the inherited name-only projection. Keep extension values out of the
public summary. Native MCP connection state, tool discovery, plugin inventory and
installed App eligibility remain independent. An Apps endpoint failure does not
hide valid MCP or plugin observations. Keep the current exact-thread settings
guard and directory readback; a changed source invalidates the combined result.

The desktop again shows advertised capabilities, exact connector IDs and separate
enabled/callable states. A discovery failure is not rendered as an empty successful
catalog. Tool-visibility controls also include the connector ID, so equal runtime
names do not make different targets indistinguishable. Keep the current task-menu
layout, refresh wording and next-turn environment explanation.

## Complete file reconciliation

- Runtime `chief_integrations.rs`: restore extension names and Apps projection and
  failure regressions; retain the newer settings guard and both settings-change
  cases. All inherited behavior is accounted for.
- Protocol `chief_integrations.rs`: the installed-App types moved within the file;
  fields, variants and serialization stay unchanged. Restore the explicit
  extension-name comment. No protocol version change is required.
- GPUI `chief_integrations.rs`: restore capability text, connector identity and
  eligibility distinctions, and their tests. Keep the existing menu/animation
  setup for the explicit login test. Retain one Apps text renderer under its
  current name. Add only an opt-in, isolated layout fixture for visual evidence.

The shared capture binary remains open for its other inherited differences.

## Validation and limits

Three runtime tests pass, including six source/error scenarios. Four desktop tests
pass, including explicit-click-only login and the restored status distinctions.
Strict protocol/runtime/GPUI Clippy passes with all features and targets.

The locally built native capture binary produces two isolated screenshots: the top
status section and the same scrollable section after scrolling. Both screenshots were inspected for
connector IDs, disabled versus enabled state, capability names, tool discovery
failure and incomplete plugin discovery. The fixture has no service profile and
uses a disposable home. It does not perform login, install plugins or use a real
account. Signed desktop and live-provider acceptance remain separate.

At fixed cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`, app-server protocol
`v2/mcp.rs` exposes initialized server capabilities separately from tool errors.
The native `mcp_server_status` test retains `extensions.openai/settings` after tool
discovery failure. Those source contracts were read. This batch does not change
native execution or claim that the separate child human-input handoff gap is fixed.
The integration panel remains optional in the user's removal review; accurate
status and identity are required if that panel is retained. Automations stay paused.
