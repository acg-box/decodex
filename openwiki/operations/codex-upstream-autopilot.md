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
  - id: openwiki-source-f02ae41e2bd15dda85dc86b9
    resource: repo://codex-runtime.lock.json
  - id: openwiki-source-c6dbc3a3344bb9841270e6c1
    resource: repo://crates/decodex-codex/examples/schema_fingerprint.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-30c7edf894a639091ecbafc6
    resource: repo://crates/decodex-runtime/src/agent/tests/external_context.rs
  - id: openwiki-source-87eeaebc39a3d2229938f9d2
    resource: repo://scripts/macos/build_codex_source.mts
  - id: openwiki-source-0e02d239506d0285d4593872
    resource: repo://scripts/macos/bundle_codex.mts
generated: { by: "codex", at: "2026-10-12T18:37:44.705Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-13T02:44:02.282Z
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

## Qualify a source runtime

The runtime lock can select an exact official source archive. Use the existing builder and bundler described in [Runtime architecture](../architecture/runtime-architecture.md); retain the repository's stable Rust build channel.

1. Verify the selected commit, official archive URL and SHA-256 digest. The builder recreates its extracted source, invokes the upstream package builder with locked Cargo dependencies, stamps the selected commit, and checks package identity and the unchanged Cargo lock.
2. Qualify the packaged executable, Code Mode helper and `rg`. The builder signs and verifies these components. The bundler checks the executable version, generates experimental app-server schemas from that executable, validates them against the local adapter and compares the fingerprint with the lock. It records the source commit, executable digest and schema fingerprint together.
3. Exercise the affected native behavior and current local consumer before delivering a lock update. A matching schema establishes protocol structure. It does not establish saved-permission behavior, compaction, transport lifecycle or desktop acceptance. Keep source review, artifact qualification, merge, installed identity and live acceptance as separate evidence.

## Preserve tool authority during continuation

`AgentCoordinator::dispatch_input` sends direct root requests and saved user replies as user input. Delegation, scheduled wakes and capacity retries use an empty input array and a `decodex` tool output named `work_instruction`, `work_wake` or `capacity_retry`. A retry retains its native retry trigger. Rejection must not cause the application to replay this output as user input.

The current tool-output encoder supplies only `name`, `namespace` and `output`. It does not request retention across compaction. Upstream support for retention does not authorize enabling it for every continuation. Keep selective retention as a separate optional decision. A proposal must show what useful output survives actual compaction and a cold resume, confirm its tool role and absence of duplicate insertion, and describe context cost and retention limits. Save that evidence in the automation directory; the repository wiki records the ownership boundary and procedure.

## Deliver small complete batches

Use a focused signed commit and PR for each independent capability or repair. Merge when repository requirements pass and read back the resulting remote main. Do not hold completed independent work behind an unfinished broad catch-up batch. Distinguish source implementation, local tests, native fixtures, desktop acceptance, installation and release.

Keep the automation's durable cursor and pending delivery records in its own directory. Record unread upstream commits separately from known implementation gaps. Historical completion words in archived notes do not create new pending work.

Match checks to the changed behavior. Use isolated system-temporary fixtures, retain only necessary evidence and clean owned temporary resources after use. Do not create a new HOME directory for each desktop attempt. Do not launch multiple application instances merely to repeat a blocked interaction.

## Notifications and historical records

Report useful merges and actionable blockers concisely; remain quiet when nothing changes. Optional proposals require a user decision before implementation. When enabled by the current operator decision, use the configured daily schedule. After about 30 minutes of active work, stop taking new work and save a continuation; this is not a hard process timeout. Record pending CI for the next run rather than waiting indefinitely.

The automation directory's `state.json` owns the current reviewed cursor, next unreviewed commit, pending adaptations, PRs and separate baseline-audit queue. Apply the latest explicit operator decision; neither old pause notes nor old resumption notes determine current host state. Complete or repair existing PRs first; avoid overlapping runs and duplicate implementation.
