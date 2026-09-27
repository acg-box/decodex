# Native child approval and MCP qualification

Restore the inherited child approval, root-tier propagation and MCP input
fixtures. Retain the current native descendant inspection assertions. Keep one
local Responses fixture and restore the exact synthetic MCP server.

## Passing qualification

The installed native child approval test passes. The pending approval retains
the child's native thread identity while its durable event belongs to the Chief.
Decline resolves it once. Native descendant inspection identifies the parent and
preserves the current child input capability. Initial child inference inherits
priority tier, model and effort. Clearing the root tier affects the existing
child's next request without changing its model or effort or rebinding the root.

Five existing local child-ownership tests and strict all-feature, all-target
runtime Clippy also pass. Fixed upstream `subagent_service_tier.rs` distinguishes
root routing policy from child-owned settings and checks subsequent requests.
The relevant source was read, not executed here.

## Installed-native mismatch: not qualified

The restored MCP fixture fails for both explicit user-input markers:

- `codex_approval_kind: browser_auth`
- `codex_requires_user_input: true`

Both synthetic empty-schema requests receive `result.action: accept` with empty
content. No local pending prompt is registered, but this is not the required
handoff result. The fixture still requires error `-32603` and guidance to the root
thread. It now evaluates both markers and reports each native reply; its strict
assertions are not relaxed. The test remains explicitly opt-in, as inherited.
A passing ordinary test run or CI does not qualify this native contract.

Observed executable:

- Version: `codex-cli 0.158.0-alpha.2`.
- SHA-256: `c3e30211bd454da70ceb4d9cbc2e05fe6466812ab05c311c3bbff6addeb14202`.
- Path: `/Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex`.

At fixed cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`codex-rs/codex-mcp/src/elicitation.rs` checks these markers before automatic
approval when user interaction is disabled. It rejects them with the root-thread
handoff message. The core subagent elicitation test also distinguishes human
input from an automatically approvable permission request. These source contracts
contradict the observed result; the cause of the installed-binary difference is
not established by this fixture.

The fixture uses disposable homes, loopback model responses and a synthetic MCP
server. It supplies no real credentials and performs no real browser login. No
installed executable, product configuration or production policy is changed.
The application does not receive the internally accepted elicitation, so this
batch does not claim a local enforcement fix or add a second MCP approval owner.

Keep all three inherited-file rows open for final native compatibility review.
The child approval result is positive; the human-input handoff result is a known
qualification failure. Re-run the strict fixture against the eventual delivery
binary before claiming the fixed-cutoff behavior. Include this limit in the final
capability review; signed desktop and broader child acceptance remain open.

## Recheck on Codex 0.158.0-alpha.2.1

The installed executable changed independently of this task. Re-run the same
strict fixture without changing its assertions or synthetic server:

- Version: `codex-cli 0.158.0-alpha.2.1`.
- SHA-256: `3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
- Log: `/tmp/decodex-child-mcp-alpha-2-1.log`.

Both markers again return `result.action: accept` and empty content. The test
fails with two failed marker cases. The fixture shuts down each native process
and aborts its local response server before reporting the failure. The cause
remains unqualified, and the three inherited-file rows remain open. This newer
binary must not be treated as a fix based on its version alone.
