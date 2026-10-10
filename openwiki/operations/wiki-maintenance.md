---
type: Reference
title: "Wiki and evidence maintenance"
description: "Current documentation ownership, dated archives, the OpenWiki lifecycle and scheduling boundaries."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
generated: { by: "codex", at: "2026-09-29T06:24:17.023Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-10T06:49:11.265Z
---

# Wiki maintenance

`openwiki/` is the only maintained project documentation tree. Source and tests define implemented behavior. Organize pages around reader tasks and system ownership, not source folders, individual commits or the number of retired documents.

## Generate and consolidate

Start an OpenWiki update, inspect current sources, and submit a focused topic plan. Reuse an existing page when it already owns the topic. Merge repeated explanations. Create a new page only when it has a distinct responsibility and useful navigation links.

Consume the assigned page jobs in order. Research and write the assigned page, reconcile its Claims through the tool, then submit it. Keep stable Claim identities when the proposition is unchanged. Correct or retract obsolete propositions rather than carrying them forward as apparent current requirements.

OpenWiki owns page indexes, source provenance, Claim sidecars and run metadata. Do not hand-edit those outputs. Finish the run and check relative links and source references before reporting completion. Generated verification metadata does not mean that every cited test or live workflow ran again.

## Retired material

Do not copy an old documentation tree into the wiki one file at a time. Extract useful current behavior into the relevant topic and verify it against source. Old plans, experiments and acceptance receipts remain in Git history. Link an exact historical revision only when its evidence is needed; do not retain a new archive copy merely to preserve file count.

Root README and policy files are concise entrypoints. Temporary task logs stay outside maintained documentation. A past completion report proves only the named revision and environment, not the current installed application.

## Scheduling

The checked-in portfolio defines an upstream Maintainer, Content Manager and Xurl Publisher. It has no dedicated Wiki role. An old generated setup sentence is not evidence of an active scheduled Wiki workflow. Verify scheduler registration separately before claiming automatic refresh.

See [Quickstart](../quickstart.md), [Commands and validation](commands-and-validation.md) and [Upstream maintenance](codex-upstream-autopilot.md).
