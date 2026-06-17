---
type: "Decision"
title: "Static Public Site"
description: "Should the public Decodex site become a dynamic app now that the runtime and"
status: active
authority: rationale
owner: docs
tags: [decision]
last_verified: 2026-06-16
---
# Static Public Site

Status: accepted

Date: 2026-05-09

Question: Should the public Decodex site become a dynamic app now that the runtime and
site live in one workspace?

Decision: Keep the public site static by default. `site/` remains an Astro static site
for the public Decodex product surface and app download entry. Runtime orchestration,
local operator state, tracker writes, app-server integration, account pools, and the
operator dashboard stay in `apps/decodex/` and the local `decodex serve` control plane.

Consequences:

- Public site deployment remains cacheable and deployable through GitHub Pages without
  a live Decodex daemon.
- `apps/decodex/` can evolve the runtime without turning the public website into an
  operational dependency.
- Upstream monitoring and public publishing automation are not part of this repository's
  public-site boundary.
- Dynamic public capabilities such as login, personalized views, live queries, or
  paid/private access require a later decision before the site depends on a backend.
