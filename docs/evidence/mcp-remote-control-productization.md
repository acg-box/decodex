---
type: "Drift Audit"
title: "MCP Remote Control Productization"
description: "Audit Streamable HTTP remote-control docs against code, tests, and external MCP security guidance."
status: active
authority: evidence
owner: docs
tags: [mcp, remote-control, semantic-drift, evidence]
source_refs: [https://modelcontextprotocol.io/specification/2025-11-25/basic/transports, https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization, https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices, https://modelcontextprotocol.io/specification/draft/changelog, https://www.nsa.gov/Portals/75/documents/Cybersecurity/CSI_MCP_SECURITY.pdf?ver=bmgiSbNQLP6Z_GiWtRt6bg%3D%3D]
code_refs: [apps/decodex/src/cli.rs, apps/decodex/src/mcp.rs, apps/decodex/tests/mcp_stdio.rs, README.md, docs/spec/runtime.md, docs/runbook/mcp-remote-control.md, docs/reference/operator-control-plane.md]
related: [../spec/runtime.md, ../runbook/mcp-remote-control.md, ../reference/operator-control-plane.md, ../decisions/mcp-capability-gateway-and-skill-slimming.md]
drift_watch: [decodex mcp serve --transport streamable-http, --allow-origin, --bearer-token-env, Authorization, Mcp-Session-Id, text/event-stream, decodex_observe, decodex_lane_control, decodex_project_control, authorization, protected-resource, process-level smoke]
last_verified: 2026-06-18
---

# MCP Remote Control Productization

## Watched Claims

- Streamable HTTP binds to loopback by default and defaults to `observe`.
- Streamable HTTP validates browser `Origin`, issues `Mcp-Session-Id` on
  `initialize`, requires known sessions after initialization, and supports JSON or
  SSE responses under stable MCP `2025-11-25`.
- `Mcp-Session-Id` and `--allow-origin` are not Decodex authorization boundaries.
- Direct non-loopback exposure requires both `--allow-origin` and
  `--bearer-token-env`; elevated Streamable HTTP profiles require
  `--bearer-token-env` even on loopback.
- The built-in bearer guard protects Decodex direct listeners but is not OAuth
  Protected Resource Metadata.
- Public-safe observation excludes hidden reasoning, private evidence payloads, host
  paths, secret material, raw steer text, and Program graph identifiers.
- Standalone MCP keeps `scan`, `manual_attention`, and `retained_resume` routed to
  canonical operator, tracker, or runtime paths instead of adding shortcuts.
- Process-level Streamable HTTP smoke coverage starts the real binary and verifies
  initialize, observe-profile tool discovery, above-profile refusal, SSE progress,
  and stdout/stderr cleanliness.

## Evidence Anchors

- `apps/decodex/src/cli.rs` exposes `--allow-origin`, `--bearer-token-env`,
  `--listen-address`, `--transport streamable-http`, and `--capability-profile`.
- `apps/decodex/src/mcp.rs` implements Streamable HTTP origin checks, bearer
  challenge/validation, non-loopback and elevated-profile startup guards, session
  handling, JSON/SSE framing, capability-profile filtering, observe resources,
  lane-control refusals, project-control `scan` refusal, and public-text guards.
- `apps/decodex/tests/mcp_stdio.rs` covers stdio process smoke and a real
  `decodex mcp serve --transport streamable-http` child-process smoke.
- `docs/decisions/mcp-capability-gateway-and-skill-slimming.md` records the selected
  staged productization strategy.
- Official MCP transport and authorization docs, MCP security guidance, the MCP draft
  changelog, and the NSA May 2026 guidance all support fail-closed remote access,
  explicit authorization boundaries, no token passthrough, and a future-compatible
  protocol seam.

## Reverse Checks

- `rg -n "bearer|Authorization|allow-origin|capability-profile|streamable-http|Mcp-Session-Id|scan|manual_attention|retained_resume" apps/decodex/src apps/decodex/tests README.md docs plugins/decodex`
  shows current CLI/MCP support, bearer authorization, and high-risk shortcut
  refusals.
- `rg -n "mcp_streamable_http_process|streamable_http_.*process" apps/decodex/tests apps/decodex/src`
  returns the real child-process Streamable HTTP smoke test.
- `cargo test -p decodex mcp::tests::streamable_http_ -- --nocapture` passes 16
  Streamable HTTP unit tests, including bearer challenge/validation and startup
  guards.
- `cargo test -p decodex --test mcp_stdio mcp_streamable_http_process_observe_profile_smoke -- --nocapture`
  passes the real child-process Streamable HTTP smoke.
- The repository validation gate that matches the touched surface must pass after this
  documentation promotion.

## Verdict

pass

The promoted docs match current implementation: direct Streamable HTTP has a bearer
boundary, non-loopback/elevated startup guards, process-level smoke coverage, and
public-safe observation. Full OAuth Protected Resource Metadata remains future
interoperability work, not a current Decodex claim.

## Required Updates

- If Decodex implements OAuth Protected Resource Metadata, update README, runtime
  spec, remote MCP runbook, operator reference, MCP tests, Decodex plugin guidance,
  and this audit in the same lane.
- When MCP authorization semantics change, keep bearer startup guards, docs, tests,
  and operator examples aligned.
- If the final MCP stateless protocol supersedes the stable 2025-11-25 session
  contract, update runtime session handling, protocol metadata, runbook examples, and
  all `Mcp-Session-Id` claims together.

## Citations

- [MCP 2025-11-25 Streamable HTTP transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)
- [MCP 2025-11-25 authorization](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)
- [MCP security best practices](https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices)
- [MCP draft changelog](https://modelcontextprotocol.io/specification/draft/changelog)
- [NSA MCP Security Design Considerations, May 2026](https://www.nsa.gov/Portals/75/documents/Cybersecurity/CSI_MCP_SECURITY.pdf?ver=bmgiSbNQLP6Z_GiWtRt6bg%3D%3D)
- [`../spec/runtime.md`](../spec/runtime.md)
- [`../runbook/mcp-remote-control.md`](../runbook/mcp-remote-control.md)
- [`../reference/operator-control-plane.md`](../reference/operator-control-plane.md)
- [`../../apps/decodex/src/mcp.rs`](../../apps/decodex/src/mcp.rs)
- [`../../apps/decodex/tests/mcp_stdio.rs`](../../apps/decodex/tests/mcp_stdio.rs)
