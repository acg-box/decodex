---
type: "Drift Audit"
title: "MCP Remote Control Productization"
description: "Audit Streamable HTTP remote-control docs against code, tests, research, and external MCP security guidance."
status: active
authority: evidence
owner: docs
tags: [mcp, remote-control, semantic-drift, evidence]
source_refs: [https://modelcontextprotocol.io/specification/2025-11-25/basic/transports, https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization, https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices, https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/, https://www.nsa.gov/Portals/75/documents/Cybersecurity/CSI_MCP_SECURITY.pdf?ver=bmgiSbNQLP6Z_GiWtRt6bg%3D%3D]
code_refs: [apps/decodex/src/cli.rs, apps/decodex/src/mcp.rs, apps/decodex/tests/mcp_stdio.rs, README.md, docs/spec/runtime.md, docs/runbook/mcp-remote-control.md, docs/reference/operator-control-plane.md, docs/research/mcp-remote-control-productization.md]
related: [../spec/runtime.md, ../runbook/mcp-remote-control.md, ../reference/operator-control-plane.md, ../decisions/mcp-capability-gateway-and-skill-slimming.md, ../research/mcp-remote-control-productization.md]
drift_watch: [decodex mcp serve --transport streamable-http, --allow-origin, Mcp-Session-Id, text/event-stream, decodex_observe, decodex_lane_control, decodex_project_control, authorization, protected-resource, process-level smoke]
last_verified: 2026-06-18
---

# MCP Remote Control Productization

## Watched Claims

- Streamable HTTP binds to loopback by default and defaults to `observe`.
- Streamable HTTP validates browser `Origin`, issues `Mcp-Session-Id` on
  `initialize`, requires known sessions after initialization, and supports JSON or
  SSE responses under stable MCP `2025-11-25`.
- `Mcp-Session-Id` and `--allow-origin` are not Decodex authorization boundaries.
- Direct non-loopback exposure and elevated Streamable HTTP profiles need an
  operator-managed authorization boundary until Decodex implements an MCP
  protected-resource auth surface.
- Public-safe observation excludes hidden reasoning, private evidence payloads, host
  paths, secret material, raw steer text, and Program graph identifiers.
- Standalone MCP keeps `scan`, `manual_attention`, and `retained_resume` routed to
  canonical operator, tracker, or runtime paths instead of adding shortcuts.
- Process-level Streamable HTTP smoke coverage and protected-resource auth remain
  productization gaps, not completed implementation evidence.

## Evidence Anchors

- `apps/decodex/src/cli.rs` exposes `--allow-origin`, `--listen-address`,
  `--transport streamable-http`, and `--capability-profile`; it does not expose a
  Decodex-owned HTTP bearer or protected-resource auth flag.
- `apps/decodex/src/mcp.rs` implements Streamable HTTP origin checks, session
  handling, JSON/SSE framing, capability-profile filtering, observe resources,
  lane-control refusals, project-control `scan` refusal, and public-text guards.
- `apps/decodex/tests/mcp_stdio.rs` covers stdio process smoke and in-process
  Streamable HTTP behavior; a real `decodex mcp serve --transport streamable-http`
  child-process smoke is still missing.
- `docs/research/mcp-remote-control-productization.md` records the external evidence
  and selected staged productization strategy.
- Official MCP transport and authorization docs, MCP security guidance, the 2026 RC
  announcement, and the NSA May 2026 guidance all support fail-closed remote access,
  explicit authorization boundaries, no token passthrough, and a future-compatible
  protocol seam.

## Reverse Checks

- `rg -n "bearer|Authorization|allow-origin|capability-profile|streamable-http|Mcp-Session-Id|scan|manual_attention|retained_resume" apps/decodex/src apps/decodex/tests README.md docs plugins/decodex`
  shows current CLI/MCP support and the absence of a Decodex-owned HTTP auth flag.
- `rg -n "mcp_streamable_http_process|streamable_http_.*process" apps/decodex/tests apps/decodex/src`
  should stay empty until the process-level HTTP smoke is implemented.
- `decodex docs check` must pass after this docs promotion.

## Verdict

pass

The promoted docs now distinguish current implementation from remaining productization
gaps. The gaps are accepted research-backed work items, not current runtime facts.

## Required Updates

- When Decodex implements HTTP protected-resource auth, update README, runtime spec,
  remote MCP runbook, operator reference, MCP tests, Decodex plugin guidance, and this
  audit in the same lane.
- When process-level Streamable HTTP smoke coverage lands, update
  `docs/reference/test-suite.md` with real test counts and update this audit with the
  exact passing command.
- If the final MCP stateless protocol supersedes the stable 2025-11-25 session
  contract, update runtime session handling, protocol metadata, runbook examples, and
  all `Mcp-Session-Id` claims together.

## Citations

- [MCP 2025-11-25 Streamable HTTP transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)
- [MCP 2025-11-25 authorization](https://modelcontextprotocol.io/specification/2025-11-25/basic/authorization)
- [MCP security best practices](https://modelcontextprotocol.io/docs/tutorials/security/security_best_practices)
- [MCP 2026-07-28 release candidate announcement](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/)
- [NSA MCP Security Design Considerations, May 2026](https://www.nsa.gov/Portals/75/documents/Cybersecurity/CSI_MCP_SECURITY.pdf?ver=bmgiSbNQLP6Z_GiWtRt6bg%3D%3D)
- [`../spec/runtime.md`](../spec/runtime.md)
- [`../runbook/mcp-remote-control.md`](../runbook/mcp-remote-control.md)
- [`../reference/operator-control-plane.md`](../reference/operator-control-plane.md)
- [`../research/mcp-remote-control-productization.md`](../research/mcp-remote-control-productization.md)
- [`../../apps/decodex/src/mcp.rs`](../../apps/decodex/src/mcp.rs)
- [`../../apps/decodex/tests/mcp_stdio.rs`](../../apps/decodex/tests/mcp_stdio.rs)
