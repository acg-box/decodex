---
type: "Runbook"
title: "MCP Remote Control"
description: "Run Decodex MCP over Streamable HTTP with safe defaults, authorization boundaries, public-safe observation, and explicit gap routing."
status: active
authority: procedural
owner: runtime
tags: [mcp, remote-control, operator, runbook]
source_refs: [https://modelcontextprotocol.io/specification/2025-11-25/basic/transports, https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization, https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices, https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/]
code_refs: [apps/decodex/src/cli.rs, apps/decodex/src/mcp.rs, apps/decodex/tests/mcp_stdio.rs]
related: [../spec/runtime.md, ../reference/operator-control-plane.md, ../decisions/mcp-capability-gateway-and-skill-slimming.md, ../evidence/mcp-remote-control-productization.md, ../research/mcp-remote-control-productization.md]
drift_watch: [decodex mcp serve --transport streamable-http, --capability-profile, --allow-origin, Mcp-Session-Id, decodex_observe, decodex_lane_control, decodex_project_control]
last_verified: 2026-06-18
---

# MCP Remote Control

Purpose: Run Decodex MCP over Streamable HTTP without weakening Decodex runtime,
tracker, review, landing, or lane-control authority.

Read this when: Connecting a remote MCP client, choosing an MCP capability profile,
or observing Decodex lane progress through MCP.

Not this document: MCP protocol rationale, private execution evidence inspection, or
permission to expose Decodex directly to the public internet.

## Safe Default

Start Streamable HTTP on loopback with the default `observe` profile:

```sh
decodex mcp serve --transport streamable-http \
  --listen-address 127.0.0.1:8193
```

The endpoint is `POST /mcp`. Browser clients that run from a different loopback
origin may add that exact origin:

```sh
decodex mcp serve --transport streamable-http \
  --listen-address 127.0.0.1:8193 \
  --allow-origin http://127.0.0.1:3000
```

`--allow-origin` is only CORS trust. It does not authenticate the caller. The
`Mcp-Session-Id` header issued after `initialize` is protocol state for the stable
MCP `2025-11-25` Streamable HTTP transport, not Decodex authorization.

## Remote Boundary

Use Streamable HTTP beyond loopback only when an operator-owned boundary protects the
listener before traffic reaches Decodex. Acceptable boundaries are a local tunnel, a
relay, network ACLs on a private network, or a future Decodex MCP protected-resource
authorization surface.

Do not expose `decodex mcp serve --transport streamable-http` directly on an
untrusted network with only `--allow-origin`. Treat `operate` and `admin` profiles as
remote-control profiles: they require the same external authorization boundary when
used through Streamable HTTP.

## Capability Profiles

Use the narrowest profile that fits the task:

| Profile | Use | Boundary |
| --- | --- | --- |
| `observe` | Read public-safe status and activity. | Default for Streamable HTTP. |
| `plan` | Use schema-bound research and intake planning tools. | Apply/promote modes still require explicit authority fields. |
| `operate` | Inspect, steer, or interrupt a current lane. | Requires external HTTP authorization when remote, plus inspect-first run/turn authority. |
| `admin` | Read project status or pause/resume future dispatch. | Requires external HTTP authorization when remote and explicit authority; active lanes are not killed. |

`tools/list` filters by the active profile. Calling a tool above the active profile
returns a structured `insufficient_capability_profile` refusal.

## Observation Recipe

Prefer public-safe resources before mutating tools:

- `decodex://projects/<project_id>/status_live`
- `decodex://projects/<project_id>/activity_tail`
- `decodex://projects/<project_id>/lane-control`
- `decodex://projects/<project_id>/lane_inspect/<issue>`
- `decodex://projects/<project_id>/runs/<run_id>/events`
- `decodex://projects/<project_id>/runs/<run_id>/protocol_activity`
- `decodex://projects/<project_id>/runs/<run_id>/child_agent_activity`
- `decodex://projects/<project_id>/runs/<run_id>/progress_diagnostics`
- `decodex://projects/<project_id>/pr_review_state`

The `decodex_observe` tool returns a public-safe structured projection and may be
used with MCP progress tokens. These resources and tools must not include hidden
reasoning, raw private evidence, host paths, secret material, raw steer message text,
or Program graph identifiers.

## Refusal Paths

Standalone MCP keeps high-risk shortcuts refused:

- `decodex_project_control` with `scan` refuses to the operator control loop.
- `decodex_lane_control` with `manual_attention` refuses to the issue-scoped tracker
  terminal path.
- `decodex_lane_control` with `retained_resume` refuses to the retained-lane runtime
  dispatch path.

Use the canonical Decodex runtime, tracker, review, landing, and lane-control paths
for those operations.

## Remaining Gaps

The current Streamable HTTP gateway still needs two productization gaps before direct
remote/elevated operation can be treated as complete:

- a Decodex-owned MCP protected-resource authorization surface or an explicitly
  documented relay-auth contract that satisfies the MCP authorization direction
  without token passthrough
- process-level Streamable HTTP smoke coverage that starts the real binary,
  initializes a session, lists observe-profile tools, verifies an above-profile
  refusal, and verifies SSE framing for an allowed call

## Compatibility Note

Decodex currently targets stable MCP `2025-11-25` Streamable HTTP sessions. The
2026-07-28 release candidate moves toward a stateless protocol core and authorization
hardening, but it is not the current Decodex runtime contract. Keep session handling
isolated from tool schemas and lane-control authority so a final stateless protocol
can be added later without widening Decodex authority.
