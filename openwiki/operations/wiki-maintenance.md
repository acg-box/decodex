---
type: Reference
title: "Wiki maintenance and source verification"
description: "Wiki maintenance and source verification"
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:55:18.668Z
---

# Wiki maintenance and source verification

## Update authority

OpenWiki is a documentation and evidence index, not runtime authority. Source, protocol schemas and focused tests define implemented behavior. Dated receipts prove only their captured revision and scope.

The supported host lifecycle is: resolve the Git root; begin an update; inspect current owners; submit a page plan; obtain one page job; research and write that page; submit its complete Claims; repeat; finish after the queue is complete. A source change that invalidates the plan requires a fresh plan.

Generated indexes, Claims sidecars, provenance and run state belong to OpenWiki. Do not manufacture freshness timestamps or treat a changed date as semantic validation. Reuse a Claim ID for the same proposition, revise its moved evidence, and retract claims whose owners no longer exist.

## Automation status

At the start of this refresh, the repository had no checked-in OpenWiki GitHub Actions workflow, and the checked-in five-task automation portfolio had no Wiki role. An old AGENTS.md setup block said a scheduled workflow refreshed the Wiki; that comment alone was not evidence of an active schedule.

OpenWiki's finish/setup mechanism may refresh its own managed integration files. Inspect the resulting workflow and GitHub registration before claiming automation is enabled or has run. This update must not independently invent a scheduler or edit managed setup blocks.

## Full refresh scope

Current pages cover the unified service, SQLite, Chief coordination, native desktop glass, subscription voice, account login/routing, Reset Cards, quota activation, build/validation, and auxiliary tools. Historical Program, server-store, private-artifact and proof receipts remain clearly scoped archives. Their old commands and binary identities are not current instructions.

For each update, check:
- entrypoints, public operation names, protocol versions and migration boundaries;
- source paths and link targets;
- transient versus durable state and positive outcome evidence;
- current versus retired product surfaces;
- tests that support each Claim, without pretending those tests ran during documentation generation.

Use [Commands and validation](commands-and-validation.md) for executable checks and [Quickstart](../quickstart.md) for navigation. A completed Wiki run, a Git commit, a merged PR and a deployed application are separate outcomes.
