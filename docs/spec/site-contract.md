---
type: "Spec"
title: "Site Contract"
description: "Define the page and route contract for the GitHub-first Decodex MVP."
status: active
authority: normative
owner: runtime
tags: [spec]
last_verified: 2026-06-16
---
# Site Contract

Purpose: Define the static public-site contract for Decodex.

Status: normative

Read this when:
- You are changing the Astro public site.
- You are implementing homepage sections or public assets.
- You need to know what the site is allowed to depend on.

Not this document:
- The local operator dashboard served by `decodex serve`.
- The Decodex runtime contract.
- Upstream monitoring or public publishing automation.

Defines:
- Allowed public routes.
- Required homepage obligations.
- Static-site dependency boundaries.

## Route Budget

The required public route is `/`.

The site may add small secondary static routes only when they support the public Decodex
product surface. The site must not add workflow dashboards, live runtime pages, hosted
operator controls, monitoring feeds, or publishing queues.

## Homepage Obligations

The homepage must present Decodex as repo-native control software for Codex work. It
must include:

- a primary Decodex brand signal in the first viewport
- a short positioning line for the runtime and retained-lane control plane
- a path to the GitHub repository
- a path to the runtime or documentation surface
- a static app download entry for the Codex beta appcast when that widget remains
  available

The homepage may explain product surfaces such as the Rust CLI, macOS app, local
operator HTTP surface, and installable plugin. It must not present legacy
upstream-monitoring feeds or external-publication workflows as repository-owned
Decodex site content.

## Static Boundary

The public site is static. It must not depend on a live Decodex daemon, runtime SQLite
state, tracker credentials, ChatGPT account-pool state, or local operator evidence.

The local operator dashboard remains owned by `decodex serve`; the public site must not
reuse dashboard routes or imply hosted operator access.

## Asset Boundary

The site may use checked-in public assets under `site/public/` and source files under
`site/src/`. Generated local build outputs such as `site/dist/` and `site/.astro/` are
not source authority and must not be treated as tracked content.
