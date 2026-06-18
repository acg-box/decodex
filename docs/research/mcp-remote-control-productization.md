---
type: "Research Contract"
title: "MCP Remote Control Productization"
description: "Research the best next steps for Decodex MCP remote control, observation, authorization, and future protocol compatibility."
status: active
authority: non_authoritative
owner: research
tags: [research, mcp, remote-control, security, operator]
source_refs: [https://modelcontextprotocol.io/specification/2025-11-25/basic/transports, https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization, https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices, https://modelcontextprotocol.io/specification/draft/changelog, https://www.nsa.gov/Portals/75/documents/Cybersecurity/CSI_MCP_SECURITY.pdf?ver=bmgiSbNQLP6Z_GiWtRt6bg%3D%3D]
code_refs: [apps/decodex/src/mcp.rs, apps/decodex/tests/mcp_stdio.rs, README.md, docs/spec/runtime.md, docs/reference/operator-control-plane.md, docs/runbook/mcp-remote-control.md, docs/evidence/mcp-remote-control-productization.md, docs/decisions/mcp-capability-gateway-and-skill-slimming.md]
related: [index.md, ../decisions/mcp-capability-gateway-and-skill-slimming.md, ../reference/operator-control-plane.md, ../spec/runtime.md, ../runbook/mcp-remote-control.md, ../evidence/mcp-remote-control-productization.md]
promotes_to: [docs/decisions, docs/spec, docs/runbook, docs/reference, docs/evidence]
last_verified: 2026-06-18
---

# MCP Remote Control Productization

Purpose: Decide the best next Decodex MCP work after the complete local and
Streamable HTTP gateway landed.

Read this when: Planning MCP remote access, operator observation, auth, smoke tests,
or follow-on control tools. Bearer direct-listener auth and process-level HTTP smoke
have already been promoted; this concept remains active for OAuth Protected Resource
Metadata, operator-loop-hosted scan, and future protocol compatibility.

Not this document: Current runtime authority or permission to implement. This
research is latent until promoted.

## Question

What remaining MCP gaps should Decodex address next, and what architecture is safest
and most future-compatible for remote control?

## Scope

In scope:

- Remote access productization for `decodex mcp serve --transport streamable-http`.
- Authentication and relay/tunnel requirements beyond loopback development.
- Observation experience for current/recent lane progress without hidden reasoning.
- Whether to expand `decodex_project_control.scan`, `manual_attention`, or
  `retained_resume`.
- End-to-end smoke coverage for real MCP HTTP process behavior.
- Compatibility with the current stable MCP specification and the MCP draft stateless
  protocol direction.

Out of scope:

- Replacing the existing MCP gateway.
- Exposing hidden chain-of-thought, private evidence, local paths, credentials, raw
  steer messages, Program graph ids, or direct DAG editing.
- Public internet exposure without an operator-managed authentication boundary.
- One-tool-per-CLI-command expansion.

## Evidence

| ID | Class | Sources | Supports |
| --- | --- | --- | --- |
| E1 | external_source | MCP 2025-11-25 Streamable HTTP transport | Streamable HTTP uses a single endpoint with POST and GET, optional SSE, and multiple client connections. The spec requires Origin validation, recommends localhost binding for local servers, and recommends authentication for all connections. |
| E2 | external_source | MCP 2025-11-25 authorization | Protected MCP servers are OAuth 2.1 resource servers. MCP servers must expose OAuth Protected Resource Metadata and clients must use that metadata to discover authorization servers. |
| E3 | external_source | MCP security best practices | MCP security guidance calls out confused deputy, token passthrough, SSRF, session hijacking, local server compromise, and scope minimization risks. Token passthrough is not an acceptable auth model for protected servers. |
| E4 | external_source | NSA MCP Security Design Considerations, May 2026 | MCP adoption has reached production AI automation, but its security model requires implementation rigor, validation, and secure-by-default behavior because agentic tool execution changes the trust pattern. |
| E5 | external_source | MCP draft changelog | The draft direction moves remote MCP toward a stateless protocol core and protocol authorization hardening, but draft behavior is not Decodex's current runtime contract. |
| E6 | repo_source | `README.md`, `docs/spec/runtime.md`, `docs/reference/operator-control-plane.md` | Decodex already exposes stdio and Streamable HTTP, defaults Streamable HTTP to loopback `observe`, validates Origin, uses sessions for MCP 2025-11-25, requires bearer authorization for non-loopback or elevated Streamable HTTP, filters tools by capability profile, and keeps remote control inspect-first. |
| E7 | repo_source | `apps/decodex/src/mcp.rs` | Current tools are deliberately small: observe, plan, research compile/promote, intake goal, lane control, and project control. `scan` refuses because standalone MCP serve cannot enqueue the operator loop request. |
| E8 | repo_source | `apps/decodex/src/mcp.rs`, `apps/decodex/tests/mcp_stdio.rs`, `docs/reference/test-suite.md` | Existing tests cover resources, templates, prompts, tool schemas, profile refusals, Streamable HTTP CORS/session/SSE behavior, bearer challenge/validation, non-loopback/elevated startup guards, process-level Streamable HTTP smoke, lane steer/interrupt preconditions, project pause, scan refusal, and stdio/stdout cleanliness. |
| E9 | inference | E1, E2, E3, E5, E6 | The current Streamable HTTP design is correct for loopback and bearer-protected direct listeners, but it should still avoid entrenching session semantics that the draft protocol may remove. |
| E10 | repo_source | `apps/decodex/src/cli.rs`, `apps/decodex/tests/mcp_stdio.rs`, `docs/evidence/mcp-remote-control-productization.md` | Current Decodex exposes `--allow-origin`, `--bearer-token-env`, and capability profiles, and has process-level Streamable HTTP child-process smoke coverage. It does not expose OAuth Protected Resource Metadata. |
| E11 | inference | E1, E2, E3, E4, E10 | The current built-in bearer guard is the right minimum direct-listener boundary, but first-class OAuth MCP client discovery should use OAuth Protected Resource Metadata or an operator-managed relay rather than token passthrough. |

## Options

1. Keep current gateway and only write usage docs.
   This is low risk and enough for local power users, but it leaves remote operators
   guessing about auth, relay, and smoke-test expectations.

2. Expose more MCP tools directly, including scan, manual attention, retained resume,
   and direct closeout controls.
   This improves apparent capability coverage but conflicts with Decodex authority
   boundaries. These controls have canonical runtime, tracker, or operator-loop paths
   that should not be bypassed by standalone MCP.

3. Productize remote MCP in stages while keeping the tool catalog small.
   The first promoted stages now exist: operator-facing remote connection docs,
   end-to-end Streamable HTTP process smoke coverage, a static bearer direct-listener
   boundary before non-loopback or elevated usage, and a public-safe observation
   guide. Keep risky controls refused unless they are backed by their canonical
   runtime path.

4. Wait for the MCP draft stateless protocol before doing more.
   This avoids rework around sessions, but it delays useful operator documentation,
   observation, and security hardening that remain valid under either protocol.

## Judgment

Selected option: Productize remote MCP in stages while keeping the tool catalog small.

Recommended sequence:

1. Promote remote MCP docs first. Done.
   Keep README, runtime spec, operator reference, decision record, runbook, evidence,
   research, and plugin routing aligned. The promoted docs must state that
   `--allow-origin` is CORS trust, not authentication; that `Mcp-Session-Id` is
   protocol state; and that direct remote/elevated Streamable HTTP requires an
   operator authorization boundary until Decodex owns protected-resource auth.

2. Add a remote-auth decision/spec before any direct non-loopback or elevated
   Streamable HTTP guidance. Done for Decodex direct listeners.
   Direct listeners now require `--bearer-token-env` for non-loopback and for
   Streamable HTTP profiles above `observe`. The docs label this as a static bearer
   guard, not OAuth Protected Resource Metadata. Future OAuth discovery should add
   Protected Resource Metadata or use a documented operator-managed relay, with no
   token passthrough.

3. Add process-level Streamable HTTP smoke coverage. Done.
   Mirror the existing stdio smoke with a real `decodex mcp serve --transport
   streamable-http` child process, initialize a session, call `tools/list` under
   `observe`, verify an above-profile refusal, and verify SSE progress on an allowed
   tool. This proves CLI wiring, listener behavior, headers, stdout/stderr
   cleanliness, and JSON/SSE framing together.

4. Improve observation without exposing reasoning.
   Keep hidden reasoning out. Provide a documented "watch" recipe that reads
   `status_live`, `activity_tail`, run events, protocol activity, child-agent
   activity, progress diagnostics, and PR/review state. If a future prompt is added,
   it should only compose these public-safe resources.

5. Keep `scan`, `manual_attention`, and `retained_resume` refused in standalone MCP.
   `scan` can be reconsidered only as an operator-loop queued request when MCP is
   hosted by or authenticated to the running operator control plane. `manual_attention`
   and `retained_resume` should stay routed to the canonical tracker/runtime
   lifecycle unless a later design proves the same terminal-state and audit
   guarantees through MCP.

6. Add a protocol-compatibility seam for the MCP draft stateless direction.
   Do not implement draft behavior as current behavior before finalization.
   Instead, isolate session handling, protocol version negotiation, and capability
   discovery so the future stateless request model can be added without changing
   Decodex authority rules or tool schemas.

Gap status after 2026-06-18 MCP bearer/smoke promotion:

- Remote runbook, runtime/reference docs, decision rationale, evidence audit, and
  plugin routing are promoted as documentation authority.
- Built-in bearer auth and process-level Streamable HTTP smoke are implemented and
  validated.
- Full OAuth Protected Resource Metadata remains future interoperability work, not a
  current Decodex runtime claim.
- `scan`, `manual_attention`, and `retained_resume` remain intentionally refused in
  standalone MCP unless a later design proves the same canonical runtime/tracker
  guarantees.

## Challenge

Resolved objection: Adding auth before remote usage may slow down dogfooding.

Resolution: Loopback plus an operator-chosen local tunnel remains useful for
dogfooding, but non-loopback operate/admin guidance without a protected-resource or
relay-token contract would contradict MCP security guidance and Decodex's authority
model.

Resolved objection: A direct `scan` tool would be convenient.

Resolution: Convenience is not enough. Current `scan` refusal is correct for
standalone `decodex mcp serve` because scan is an in-memory operator-loop request.
The safe version is an operator-loop-backed request, not a standalone MCP shortcut.

Resolved objection: Operators want to see agent thinking.

Resolution: Decodex should expose public-safe progress and protocol activity, not
hidden reasoning. The best next UX is better resource composition and watch recipes,
not private transcript exposure.

Resolved objection: The MCP draft stateless direction may invalidate current
Streamable HTTP sessions.

Resolution: The current implementation targets stable 2025-11-25 correctly. The
right hedge is a compatibility seam and smoke coverage around session behavior, not
waiting or prematurely replacing the stable transport with draft behavior.

## Decision

Terminal status: `decision_ready`.

Decision: The next MCP work should be a staged productization plan:

- document remote use first
- require bearer auth before non-loopback or elevated Streamable HTTP
  recommendations
- add process-level Streamable HTTP smoke coverage
- improve public-safe observation recipes
- keep high-risk shortcuts refused until they can route through canonical operator,
  tracker, or runtime authority
- isolate protocol-session handling for the upcoming stateless MCP direction

The bearer and smoke portions were promoted and implemented. The remaining research
continues only for OAuth Protected Resource Metadata, operator-loop-hosted scan, and
future protocol compatibility.

## Promotion

Promotion target: `docs/decisions`, `docs/spec`, `docs/runbook`, `docs/reference`,
and `docs/evidence`.

If accepted, promote as:

- `docs/decisions/`: a durable remote MCP productization decision.
- `docs/spec/runtime.md`: auth/relay, profile, public-safe observation, and
  protocol-compatibility requirements.
- `docs/runbook/`: remote MCP connection and observation recipe.
- `docs/reference/test-suite.md` and/or `docs/evidence/`: process-level Streamable
  HTTP smoke evidence after implementation.
- `apps/decodex/src/mcp.rs` and tests: bearer guard and process smoke after explicit
  implementation authority.

OKF disposition: `continue`.

## Drift Impact

- MCP docs that mention remote access, Streamable HTTP, sessions, or capability
  profiles should distinguish stable 2025-11-25 behavior from draft stateless MCP
  behavior.
- Any future OAuth Protected Resource Metadata implementation must update README,
  runtime spec, operator-control reference, tests, and plugin guidance together.
- Any future tool expansion must be checked against the "small catalog" rule and
  existing Decodex authority gates.
- Any future observation UX must preserve public-safe redaction guarantees.

## Citations

- [MCP 2025-11-25 Streamable HTTP transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)
- [MCP 2025-11-25 authorization](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)
- [MCP security best practices](https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices)
- [MCP draft changelog](https://modelcontextprotocol.io/specification/draft/changelog)
- [NSA MCP Security Design Considerations, May 2026](https://www.nsa.gov/Portals/75/documents/Cybersecurity/CSI_MCP_SECURITY.pdf?ver=bmgiSbNQLP6Z_GiWtRt6bg%3D%3D)
- [`README.md`](../../README.md)
- [`../spec/runtime.md`](../spec/runtime.md)
- [`../reference/operator-control-plane.md`](../reference/operator-control-plane.md)
- [`../../apps/decodex/src/mcp.rs`](../../apps/decodex/src/mcp.rs)
