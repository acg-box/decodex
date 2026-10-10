---
type: Reference
title: "Codex upstream maintenance"
description: "Incremental upstream review against retained consumers, native ownership and focused delivery."
tags: ["decodex", "architecture"]
openwiki_generated: true
sources:
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
  - id: openwiki-source-7dcbb082d2502f1ec4386c39
    resource: repo://automations/upstream/prompts/maintainer.md
generated: { by: "codex", at: "2026-10-10T06:49:11.265Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-10T06:49:11.265Z
---


# Codex upstream maintenance

## Desired registration and authority

The checked-in portfolio contains one upstream Maintainer, plus the separate Content Manager and Xurl Publisher. The earlier upstream Reviewer and Health role descriptions are historical. The repository default configures the upstream Maintainer as **PAUSED** with worktree execution, and points to `automations/upstream/prompts/maintainer.md`. A repository definition is not proof of host registration or execution. Verify host state through supported automation tools before changing it.

The checked-in prompt requires the automation to remain paused during manual catch-up. A completed catch-up or documentation update does not authorize resumption; that requires an explicit user instruction. When maintenance is authorized to run, existing-capability fixes may be implemented and merged after required checks; new product capabilities still require a user decision. Content publication has separate scope and is not part of upstream compatibility work.

## Review the delta against real consumers

Start from current Decodex main and inspect merged changes, open PRs and related active work. Record the official upstream revision, the installed Codex binary and its actual supported schema separately. Development follows official upstream main, not stable tags. Freeze the exact main commit for each review and qualification batch, and qualify the matching runtime artifact, helpers, provenance and schema before updating the lock. A prerelease label alone is not a reason to wait or ask again. Read consecutive incremental batches from a verified cursor. An upstream commit is discovery evidence, not automatic permission to add a product feature.

Read the changed source and tests, then trace each change to current consumers. A title, path classification or schema alone does not establish compatibility. Record tests read separately from tests executed. Leave unresolved source, tests or impact unreviewed; do not advance the contiguous cursor past those items. Keep pending implementation separate from completed review.

Classify each relevant change:

| Classification | Action |
| --- | --- |
| Existing consumer compatibility or correctness | Explain the concrete failure, adapt the current owner and validate the affected behavior |
| Native-owned behavior with no local adaptation | Record applicability; do not duplicate native execution or policy |
| New control, workflow, dashboard or automatic policy | Propose value, scope and maintenance cost; wait for the user's selection |
| Unsupported installed binary or unrelated platform | Record the boundary and defer the local consumer |

Use the [current product scope](../decisions/upstream-product-scope.md). Retired local plugin management, embedded HTML widgets and provider recovery recording must not return merely because upstream supports them.

## Deliver small complete batches

Use a focused signed commit and PR for each independent capability or repair. Merge when repository requirements pass and read back the resulting remote main. Do not hold completed independent work behind an unfinished broad catch-up batch. Distinguish source implementation, local tests, native fixtures, desktop acceptance, installation and release.

Keep the automation's durable cursor and pending delivery records in its own directory. Record unread upstream commits separately from known implementation gaps. Historical completion words in archived notes do not create new pending work.

Match checks to the changed behavior. Use isolated system-temporary fixtures, retain only necessary evidence and clean owned temporary resources after use. Do not create a new HOME directory for each desktop attempt. Do not launch multiple application instances merely to repeat a blocked interaction.

## Notifications and historical records

Report useful merges and actionable blockers concisely; remain quiet when nothing changes. Optional proposals require a user decision before implementation. When enabled by the current operator decision, use the configured daily schedule. After about 30 minutes of active work, stop taking new work and save a continuation; this is not a hard process timeout. Record pending CI for the next run rather than waiting indefinitely.

The automation directory's `state.json` owns the current reviewed cursor, next unreviewed commit, pending adaptations, PRs and separate baseline-audit queue. Apply the latest explicit operator decision; neither old pause notes nor old resumption notes determine current host state. Complete or repair existing PRs first; avoid overlapping runs and duplicate implementation.
