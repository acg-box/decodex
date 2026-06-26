---
type: "Decision"
title: "Radar, Control Plane, and Publisher"
description: "Record the decision to describe Decodex as one product across Radar, Control Plane, and Publisher."
status: active
authority: rationale
owner: docs
tags: [decision, radar]
last_verified: 2026-06-25
---
# Radar, Control Plane, and Publisher

Status: accepted

Date: 2026-05-13

Question: How should the integrated Decodex repository describe the two merged
capability sets and the new public publishing workflow?

Decision: Treat Decodex as one product with three named capability areas:

- **Radar**: upstream Codex change intelligence. Radar owns GitHub bundle collection,
  release-delta evidence, code-aware editorial analysis, and upstream impact triage.
- **Control Plane**: repo-native retained agent orchestration. Control Plane owns
  registered projects, app-server integration, tracker writes, local runtime state,
  operator status, review handoff, landing, closeout, and cleanup.
- **Publisher**: public static-site and social publishing surfaces. Publisher consumes
  Radar outputs and produces checked-in signal entries, release-delta content, and
  low-frequency social publication records for external publication.

The temporary A/B repository labels are discussion aids only. Use the capability names
above in new documentation, issue text, schema names, and operator-facing language.

Consequences:

- Radar can improve Control Plane without coupling the public site to the runtime.
  Upstream Codex changes that touch app-server, plugins, browser automation, MCP,
  permission profiles, config, or sandbox behavior should be classified for Control
  Plane impact before they become engineering work.
- `upstream_impact/v1` is the shared Radar handoff for downstream Publisher and
  Control Plane self-iteration. Release deltas, compare metadata, reviews, and URLs
  remain provenance and gap evidence, but new Radar-derived publication candidates and
  Control Plane upgrade candidates should reuse the same impact conclusion.
- Publisher remains static-first. Public pages and social publication records are
  generated from checked-in artifacts and reviewed content, not from a live Decodex
  daemon.
- `@decodexspace` content should not duplicate a release bot. Publisher should turn
  Radar evidence into practical, evidence-backed user and operator angles.
- Control Plane remains the local execution authority. Publisher content may describe
  Decodex implications, but it must not claim shipped runtime behavior unless the
  relevant code, docs, or release evidence exists.
