---
type: Reference
title: "Codex upstream maintenance"
description: "Paused upstream maintenance, incremental consumer review, optional feature decisions and focused delivery."
tags: ["decodex", "architecture"]
openwiki_generated: true
verified:
  - by: openwiki/0.6.0
    at: 2026-09-28T06:22:45.949Z
sources:
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
  - id: openwiki-source-7dcbb082d2502f1ec4386c39
    resource: repo://automations/upstream/prompts/maintainer.md
  - id: openwiki-source-004176ccd86bf2ef039677bc
    resource: repo://docs/archive/upstream-2026-09/coverage-readback-20260928.json
  - id: openwiki-source-8d4b61fd83ed007c18390abe
    resource: repo://docs/archive/upstream-2026-09/upstream-feature-decisions.md
generated: { by: "codex", at: "2026-09-28T06:22:45.949Z" }
---


# Codex upstream maintenance

## Current registration and authority

The checked-in portfolio contains one upstream Maintainer, plus the separate Content Manager and Xurl Publisher. The earlier upstream Reviewer and Health role descriptions are historical. The upstream Maintainer is configured **ACTIVE**, uses its own worktree, and points to `automations/upstream/prompts/maintainer.md`. A repository definition is not proof of host registration or execution. Verify host state through supported automation tools before changing it.

The user explicitly authorized resumption on 2026-09-28. Existing-capability fixes may be implemented and merged after required checks; new product capabilities still require a user decision. Content publication has separate scope and is not part of upstream compatibility work.

## Review the delta against real consumers

Start from current Decodex main and inspect merged changes, open PRs and related active work. Record the official upstream revision, the installed Codex binary and its actual supported schema separately. Read consecutive incremental batches from a verified cursor. An upstream commit is discovery evidence, not automatic permission to add a product feature.

Classify each relevant change:

| Classification | Action |
| --- | --- |
| Existing consumer compatibility or correctness | Explain the concrete failure, adapt the current owner and validate the affected behavior |
| Native-owned behavior with no local adaptation | Record applicability; do not duplicate native execution or policy |
| New control, workflow, dashboard or automatic policy | Propose value, scope and maintenance cost; wait for the user's selection |
| Unsupported installed binary or unrelated platform | Record the boundary and defer the local consumer |

The [product scope decision](../decisions/upstream-product-scope.md) retains O01–O23 and O25–O26. O24 must not be reintroduced by a future scan. Retaining existing optional features does not authorize unrelated additions.

## Deliver small complete batches

Use a focused signed commit and PR for each independent capability or repair. Merge when repository requirements pass and read back the resulting remote main. Do not hold completed independent work behind an unfinished broad catch-up batch. Distinguish source implementation, local tests, native fixtures, desktop acceptance, installation and release.

Keep the automation's durable cursor and pending delivery records in its own directory. Record unread upstream commits separately from known implementation gaps. Historical completion words in archived notes do not create new pending work.

Match checks to the changed behavior. Use isolated system-temporary fixtures, retain only necessary evidence and clean owned temporary resources after use. Do not create a new HOME directory for each desktop attempt. Do not launch multiple application instances merely to repeat a blocked interaction.

## Notifications and historical records

Report useful merges and actionable blockers concisely; remain quiet when nothing changes. Optional proposals require a user decision before implementation. Run daily on the existing schedule. After about 30 minutes of active work, stop taking new work and save a continuation; this is not a hard process timeout. Record pending CI for the next run rather than waiting indefinitely.

The [September 2026 archive](../../docs/archive/upstream-2026-09/README.md) contains the 1,569-commit fixed-cutoff review, 360-path inherited audit, PR1378 disposition and dated acceptance records. Counts are coverage evidence, not feature counts. Start new work from current sources, not obsolete stage instructions.

## Reviewed range and earlier coverage

The [2026-09-28 readback](../../docs/archive/upstream-2026-09/coverage-readback-20260928.json) verifies all 1,569 commits in `a397079287e6638b39dda329835350d93222681f..595cc91e8cbb1c2ca822d0311dcf12709410c582` against the saved inventory. The cutoff commit is dated 2026-09-19. Upstream main observed at `1cc7e2361237ce7244430ee1d581c77f95c57ac8` contained 493 subsequent commits. The observation is not a claim that those commits have been reviewed.

The earlier interval from `17df7545a34ac533eedf5b628f49f2f1ad60e44e` to `a397079287e6638b39dda329835350d93222681f` contains 583 commits but has no located complete per-commit review certificate. Existing-consumer baseline reconstruction and inherited-file preservation are separate evidence. Audit concrete retained consumer contracts in bounded batches; do not move the cursor backward or claim older coverage from file counts.

The automation directory's `state.json` owns the current reviewed cursor, next unreviewed commit, pending adaptations, PRs and separate baseline-audit queue. Historical pause and unfinished-goal notes do not override the user's newer authorization. Complete or repair existing PRs first; avoid overlapping runs and duplicate implementation.
