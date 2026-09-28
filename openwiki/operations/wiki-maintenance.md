---
type: Reference
title: "Wiki and evidence maintenance"
description: "Current documentation ownership, dated archives, the OpenWiki lifecycle and scheduling boundaries."
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.6.0
    at: 2026-09-28T02:19:36.307Z
sources:
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
  - id: openwiki-source-e7e2b18dcd23b3b9fac7753b
    resource: repo://docs/archive/upstream-2026-09/migration-map.tsv
  - id: openwiki-source-8ea98a5c00f00b259b6e3d8e
    resource: repo://docs/archive/upstream-2026-09/README.md
generated: { by: "codex", at: "2026-09-28T02:19:36.307Z" }
---


# Wiki and evidence maintenance

## One current knowledge entrypoint

OpenWiki contains current architecture, workflows, product decisions, operations and testing boundaries. Source code and focused tests define implemented behavior. A historical receipt proves only its named revision, artifact and environment. Do not treat an old “remaining work” paragraph as a current requirement.

The former `work/` collection and root scan journal are preserved in the [September 2026 archive](../../docs/archive/upstream-2026-09/README.md). Its [migration map](../../docs/archive/upstream-2026-09/migration-map.tsv) accounts for all 186 moved records with original hashes and current topic destinations. The root scan document is now a short navigation entrypoint.

## Where new material belongs

| Material | Owner |
| --- | --- |
| Current behavior, source relationships and supported workflows | The corresponding OpenWiki architecture, workflow or integration page |
| User product choices and stable tradeoffs | OpenWiki decisions |
| Commands, diagnostics and maintenance procedure | OpenWiki operations |
| Revision-specific PR reconciliation, experiments and acceptance receipts | A dated archive or evidence page with an explicit scope |
| Temporary task continuation and local logs | The task's local working area; clean after delivery |

Consolidate related explanations instead of creating a page for every commit. Do not duplicate a product contract in a new `work/` directory. Keep old records intact as history when they contain distinct evidence, and link from current pages to the relevant record. Old temporary fixture paths can be unavailable after cleanup; they must not be represented as permanent artifacts.

## OpenWiki update lifecycle

Resolve the Git root, begin an update, inspect source owners, submit a page plan and process the assigned page jobs in order. Read each existing page and its Claims before changing it. Retain stable Claim IDs for the same proposition; revise moved evidence and retract propositions the page no longer makes. Submit Claims through OpenWiki and finish only when the queue is complete.

OpenWiki owns indexes, Claim sidecars, provenance and run state. Do not manufacture verification dates or edit those files by hand. A path change requires updating affected page evidence through the same lifecycle. Generated page metadata means documentation validation, not a fresh execution of every cited test.

## Scheduling and effects

The portfolio currently has three upstream/content roles and no dedicated Wiki role. The upstream Maintainer is paused. An old setup sentence does not prove a registered scheduled refresh. This consolidation does not enable a Wiki workflow, resume automation or publish a product release.

Check relative links, source references and the migration inventory for a documentation move. Run behavioral tests only when the change affects behavior. See [commands](commands-and-validation.md), [product decisions](../decisions/upstream-product-scope.md), and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md).
