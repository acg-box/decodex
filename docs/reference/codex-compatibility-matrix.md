---
type: "Reference"
title: "Codex Compatibility Matrix"
description: "Record the current source-backed compatibility evidence between Decodex and upstream Codex CLI/app-server releases."
status: active
authority: current_state
owner: automation
tags: [reference, codex, compatibility]
source_refs: [https://github.com/openai/codex/releases, https://www.npmjs.com/package/@openai/codex]
code_refs: [apps/decodex/src/agent/app_server.rs, apps/decodex/src/radar.rs, docs/spec/app-server.md]
drift_watch: [codex --version, npm view @openai/codex version dist-tags --json, decodex probe, release_delta/v1, control_plane_upgrade_candidate/v1]
last_verified: 2026-06-27
---
# Codex Compatibility Matrix

Purpose: Record the current source-backed compatibility evidence between Decodex and
upstream Codex CLI/app-server releases.

Read this when: You need to know which upstream Codex release Decodex has been tested
against, or whether a preview release needs Radar review before Decodex adopts protocol
or app-server changes.

Not this document: The app-server protocol contract. Read
[`../spec/app-server.md`](../spec/app-server.md). The upgrade artifact shape is
[`../spec/control-plane-upgrade-candidate.md`](../spec/control-plane-upgrade-candidate.md).

## Boundary

This matrix is planning evidence, not runtime dispatch logic. Decodex must keep using
capability probes, app-server preflight, targeted tests, and `decodex probe` as the
authority for compatibility.

Do not branch behavior only on a Codex version string. Version and tag rows exist to
help Radar and operators decide what needs source review, not to bypass protocol
validation.

## Current Rows

| Decodex build | Codex channel | Codex version/tag | Evidence | Status | Caveat |
| --- | --- | --- | --- | --- | --- |
| `0.2.0-35fa270f` | stable | `0.142.2`, `rust-v0.142.2` | Local `/opt/homebrew/bin/codex --version` reported `codex-cli 0.142.2`; `npm view @openai/codex version dist-tags --json` reported `latest = 0.142.2`; `decodex probe` returned `PROBE_OK` on 2026-06-27. | compatible | Probe evidence covers the local installed stable CLI/app-server path, not every upstream change since the prior reviewed release. |
| `0.2.0-35fa270f` | preview | `0.143.0-alpha.25`, `rust-v0.143.0-alpha.25` | `npm view @openai/codex dist-tags` reported `alpha = 0.143.0-alpha.25`; `decodex radar refresh-release-delta --dry-run` identified the preview tag and a stable-to-preview compare window. | needs_review | Preview was not installed or probe-tested in this checkout. Radar must review app-server, MCP, plugin, sandbox, auth, browser, and config changes before Decodex treats the preview as compatible. |

## Review Triggers

Create or update `control_plane_upgrade_candidate/v1` when a Codex release, preview,
PR, or commit touches any of these surfaces:

- app-server protocol methods, events, JSON schema, or error behavior
- dynamic tools, plugin discovery, MCP, browser, or Chrome integration
- sandbox, approval, permission profiles, hooks, auth, or account state
- config schema, model defaults, provider behavior, or CLI flags used by Decodex
- app/server lifecycle behavior that affects `decodex serve`, `decodex app`, or
  retained lanes

Candidate creation is still evidence-only. Promotion to executable work requires the
authority bridge in
[`../spec/control-plane-upgrade-candidate.md`](../spec/control-plane-upgrade-candidate.md).
