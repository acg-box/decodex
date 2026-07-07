---
type: "Runbook"
title: "MCP Remote Control"
description: "Run Decodex MCP over Streamable HTTP with safe defaults, authorization boundaries, public-safe observation, and explicit gap routing."
status: active
authority: procedural
owner: runtime
tags: [mcp, remote-control, operator, runbook]
source_refs: [https://modelcontextprotocol.io/specification/2025-11-25/basic/transports, https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization, https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices, https://modelcontextprotocol.io/specification/draft/changelog]
code_refs: [apps/decodex/src/cli.rs, apps/decodex/src/mcp.rs, apps/decodex/tests/mcp_stdio.rs]
related: [../spec/runtime.md, ../reference/operator-control-plane.md, ../decisions/mcp-capability-gateway-and-skill-slimming.md, ../evidence/mcp-remote-control-productization.md]
drift_watch: [decodex mcp serve --transport streamable-http, --capability-profile, --allow-origin, --bearer-token-env, Authorization, Mcp-Session-Id, decodex_observe, decodex_lane_control, decodex_project_control]
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

Use Streamable HTTP beyond loopback only with both an explicit trusted origin and a
bearer token read from an environment variable:

```sh
export DECODEX_MCP_TOKEN="$(openssl rand -base64 32 | tr -d '\n')"

decodex mcp serve --transport streamable-http \
  --listen-address 0.0.0.0:8193 \
  --allow-origin https://relay.example \
  --bearer-token-env DECODEX_MCP_TOKEN
```

Clients must send `Authorization: Bearer <token>` on `POST` and `DELETE` requests.
Decodex still allows unauthenticated `OPTIONS` preflight so browser clients can
complete CORS negotiation.

Do not expose Streamable HTTP directly on an untrusted network with only
`--allow-origin`. The built-in bearer guard is the minimum direct-listener boundary;
it is not OAuth Protected Resource Metadata. Operators that need OAuth discovery,
central revocation, per-user policy, or broader MCP client interoperability should
put an OAuth-capable relay, tunnel, reverse proxy, or network ACL in front.

## Capability Profiles

Use the narrowest profile that fits the task:

| Profile | Use | Boundary |
| --- | --- | --- |
| `observe` | Read public-safe status and activity. | Default for Streamable HTTP. |
| `plan` | Use schema-bound intake planning and objective-proposal tools. | Streamable HTTP requires `--bearer-token-env`; apply modes still require explicit authority fields. |
| `operate` | Inspect, steer, or interrupt a current lane. | Streamable HTTP requires `--bearer-token-env`, plus inspect-first run/turn authority. |
| `admin` | Read project status or pause/resume future dispatch. | Streamable HTTP requires `--bearer-token-env` and explicit authority; active lanes are not killed. |

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

## Future Work

The direct-listener productization gaps are now closed by the bearer boundary and the
process-level Streamable HTTP smoke test. Remaining future work is intentionally
narrower:

- Add OAuth Protected Resource Metadata only if Decodex needs first-class OAuth MCP
  client discovery instead of a static operator bearer token or external relay.
- Add an operator-loop-hosted `scan` request only if it can preserve the same
  scheduler, tracker, and audit guarantees as `POST /api/linear-scan`.

## Compatibility Note

Decodex currently targets stable MCP `2025-11-25` Streamable HTTP sessions. The
MCP draft changelog moves toward a stateless protocol core and authorization
hardening, but draft behavior is not the current Decodex runtime contract. Keep
session handling isolated from tool schemas and lane-control authority so a final
stateless protocol can be added later without widening Decodex authority.
