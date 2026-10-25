---
type: Reference
title: "Codex upstream maintenance"
description: "Incremental upstream review against retained consumers, native ownership and focused delivery."
tags: ["decodex", "architecture"]
openwiki_generated: true
sources:
  - id: openwiki-source-c76550e85b25876b339f9255
    resource: repo://apps/decodex-gpui/src/agent_context_settings.rs
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
  - id: openwiki-source-a5b9d61b2b9a5f7492d602ac
    resource: repo://automations/upstream/current-review.json
  - id: openwiki-source-7dcbb082d2502f1ec4386c39
    resource: repo://automations/upstream/prompts/maintainer.md
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-2b08587e9b26e203156110c8
    resource: repo://crates/decodex-codex/src/app_server_client/context_preferences.rs
  - id: openwiki-source-75f737a722500e44692521ca
    resource: repo://crates/decodex-components/src/development.rs
  - id: openwiki-source-0d4c68456f2e50c6fc7ad5eb
    resource: repo://crates/decodex-components/src/install.rs
  - id: openwiki-source-2d990e4e2bc233a95b4894b9
    resource: repo://crates/decodex-components/src/pin.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-30c7edf894a639091ecbafc6
    resource: repo://crates/decodex-runtime/src/agent/tests/external_context.rs
  - id: openwiki-source-87eeaebc39a3d2229938f9d2
    resource: repo://scripts/macos/build_codex_source.mts
  - id: openwiki-source-3b57179b92b257bc3fff51a1
    resource: repo://scripts/macos/stage_decodex_app.sh
generated: { by: "codex", at: "2026-10-24T21:11:31.338Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-25T01:55:03.097Z
---


# Codex upstream maintenance

## Desired registration and authority

The checked-in portfolio contains one upstream Maintainer, plus the separate Content Manager and Xurl Publisher. The earlier upstream Reviewer and Health role descriptions are historical. The repository configures the upstream Maintainer as **ACTIVE**, with model `gpt-6.1-sol`, reasoning effort `max`, worktree execution and a daily UTC 20:05 schedule. Its prompt is `automations/upstream/prompts/maintainer.md`. A repository definition is not proof of host registration or execution. Verify host state through supported automation tools before changing it.

The checked-in prompt requires the automation to remain paused during manual catch-up. A completed catch-up or documentation update does not authorize resumption; that requires an explicit user instruction. When maintenance is authorized to run, existing-capability fixes may be implemented and merged after required checks; new product capabilities still require a user decision. Content publication has separate scope and is not part of upstream compatibility work.

## Review the delta against real consumers

Start from current Decodex main and inspect merged changes, open PRs and related active work. Record the official upstream revision, the installed Codex binary and its actual supported schema separately. Development can follow a fixed, reviewed official upstream commit before a Release exists. Formal Decodex releases use official Codex Releases that preserve the current merged baseline. Freeze the exact main commit for each review and qualification batch, and qualify the matching runtime artifact, helpers, provenance and schema before updating the lock. A prerelease label alone is not a reason to wait or ask again. Read consecutive incremental batches from a verified cursor. An upstream commit is discovery evidence, not automatic permission to add a product feature.

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

The runtime lock can select an exact official source archive. Use the explicit development preparation command and shared component store described in [Runtime architecture](../architecture/runtime-architecture.md); retain the repository's stable Rust build channel.

1. Verify the selected commit, official archive URL and SHA-256 digest. The builder recreates its extracted source with current file modification times, invokes the upstream package builder with locked Cargo dependencies, stamps the selected commit, and checks package identity and the unchanged Cargo lock. Refreshing the source timestamps prevents reuse of stale local Cargo artifacts.
2. Qualify the packaged executable, Code Mode helper and `rg`. The builder signs and verifies these components. The component store checks the executable and helper commit, CLI version, signatures and generated experimental app-server schemas against the local adapter and pinned fingerprint. It records archive and executable hashes with the fixed source requirement. The selection changes only after the prepared component verifies. App startup and packaging never compile Codex.
3. Exercise the affected native behavior and current local consumer before delivering a lock update. A matching schema establishes protocol structure. It does not establish saved-permission behavior, compaction, transport lifecycle or desktop acceptance. Keep source review, artifact qualification, merge, installed identity and live acceptance as separate evidence.

## Retain evidence after artifact cleanup

The [portable evidence index](../../automations/upstream/retained-evidence.json) keeps current retained behavior requirements, exact test entrypoints, artifact and helper identities, public delivery references and qualification limits. The linked [current source review](../../automations/upstream/current-review.json) records each reviewed behavior, source and test references, concrete consumers and validation limits. The [current qualification](../../automations/upstream/qualification.json) records the exact CLI and helpers, reusable fixture hashes and executed test identities. These current records are replaced when the baseline changes; Git retains earlier versions. The native automation directory still owns the live cursor, pending work and detailed run receipts. The index does not turn a source review, a historical test selection or a merged PR into fresh artifact qualification.

Keep the index compact and use Git history for replaced entries. Keep binaries, build trees, real account data, personal paths, native scheduler definitions and raw logs outside the repository. Preserve a required custom regression fixture with its repository test owner so that removal of a local run directory does not remove the only implementation.

Missing local products or logs do not block independent source review. Reconcile the index with current source, tests and public delivery evidence. Reconstruct only the affected proof and prepare an exact artifact again when required. Qualify the rebuilt artifact separately; do not identify it as the deleted original. Record unrecovered historical claims as unknown, scope a proof gap to the action that needs it, and retain the existing cursor and runtime baseline until their own requirements are met.

## Select an official Release

Use `DECODEX_CODEX_RELEASE=<exact-version> cargo make pin-codex` to prepare `codex-runtime.release.lock.json`. The command resolves the official tag and asset digest, verifies the downloaded package and schema, and rejects a Release that does not contain the current development baseline. Do not lower the baseline or label old code as a newer artifact to make this check pass.

The ancestry check preserves source inclusion; it does not prove behavior. Verify affected native regressions and retained fixes against the actual package and helpers before approving a Release requirement. Source review, the local development artifact, the selected Release and the installed application remain separate identities.

`DECODEX_BUILD_CHANNEL=release scripts/macos/stage_decodex_app.sh` requires the official Release requirement to cover the current baseline. Ordinary staging uses the development requirement. Both embed a requirement and use the same runtime cache logic; they do not embed Codex. See [Commands and validation](commands-and-validation.md).

## Preserve tool authority during continuation

`AgentCoordinator::dispatch_input` sends direct root requests and saved user replies as user input. Delegation, scheduled wakes and capacity retries use an empty input array and a `decodex` tool output named `work_instruction`, `work_wake` or `capacity_retry`. A retry retains its native retry trigger. Rejection must not cause the application to replay this output as user input.

The encoder adds `retain: true` only to `work_instruction` outputs. This covers compatibility initial delegation and explicit follow-up instructions. Scheduled wakes, capacity retries and other external results do not request retention. The output keeps its application tool role. Native feature configuration and the native retention budget decide whether it survives compaction; the annotation does not force preservation.

Validate selected retention against the exact native artifact. Check compaction and cold resume, tool provenance, absence of duplicate insertion, and the disabled-feature and unselected-output controls. Save the dated qualification receipt in the automation directory. This wiki records the ownership boundary and reusable procedure.

## Configure native context defaults

Task preferences > Context defaults and manual `config.toml` edits use the same native configuration. The two settings are `features.async_compaction` and `features.retain_client_tool_outputs`. Both use the native off default when no enabled value is present. A configured preference does not bypass native runtime eligibility or capacity limits.

The settings owner reads the user layer and effective project configuration, then writes one versioned boolean through native `config/batchWrite`. It preserves an existing async threshold table and distinguishes the saved user value from a higher-priority override. The write does not reload the configuration of loaded conversations; it changes defaults for new conversations. A lost reply requires readback, not automatic replay.

Background compaction can prepare work before foreground compaction needs it. Extra or discarded requests can add cost. Native fixtures establish behavior under their recorded conditions; they do not establish a real-provider latency or cost benefit. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md).

## Deliver small complete batches

Use a focused signed commit and PR for each independent capability or repair. Merge when repository requirements pass and read back the resulting remote main. Do not hold completed independent work behind an unfinished broad catch-up batch. Distinguish source implementation, local tests, native fixtures, desktop acceptance, installation and release.

Keep the automation's durable cursor and pending delivery records in its own directory. Record unread upstream commits separately from known implementation gaps. Historical completion words in archived notes do not create new pending work.

Match checks to the changed behavior. Use isolated system-temporary fixtures, retain only necessary evidence and clean owned temporary resources after use. Do not create a new HOME directory for each desktop attempt. Do not launch multiple application instances merely to repeat a blocked interaction.

## Notifications and historical records

Report useful merges and actionable blockers concisely; remain quiet when nothing changes. Optional proposals require a user decision before implementation. When enabled by the current operator decision, run once daily without a task-defined duration limit. Continue across small review and delivery batches until the fixed upstream target and its required repair, artifact qualification, PR, CI and merge work are complete. End early only when a concrete blocker requires user input or an external change. Save the exact continuation and pending CI head, preserve active processes and changes, and prevent overlapping runs.

The automation directory's `state.json` owns the current reviewed cursor, next unreviewed commit, pending adaptations, PRs and separate baseline-audit queue. Apply the latest explicit operator decision; neither old pause notes nor old resumption notes determine current host state. Complete or repair existing PRs first; avoid overlapping runs and duplicate implementation.

## Project and Work delivery evidence

The retained index has a separate `retained_project_agent_work` qualification for the direct schema56/protocol3 change and global Guardian. It records exact source, artifact/helper/schema identities, reusable tests, executed counts and unmeasured limits. It does not advance the upstream source-review cursor or certify the installed app. The accepted private Review Details decision is implemented; unrelated optional proposals keep their recorded decisions. Native automation state and its paused schedule remain separate authority. Reconcile this product delivery with current main and pending native receipts before an explicitly authorized maintenance resume.
