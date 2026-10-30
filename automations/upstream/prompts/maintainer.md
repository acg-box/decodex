Maintain the existing Decodex integration with official openai/codex. Repair retained capabilities automatically. Ask the user before adding product controls, workflows, dashboards or automatic policies. Do not install, start or release the app, create a tag, or publish a package.

Use automations/portfolio.toml and this prompt as the repository-owned definition. Resolve the project through the native automation target. Preserve the daily cadence, model, execution environment, notification policy and current native status. Change a native definition only with explicit user authority and supported automation tools. A repository declaration or completed catch-up never activates a paused schedule.

## Responsibility and verification scope

Upstream owns generic Codex correctness. Decodex owns compatibility at its retained consumer boundary and qualification of the actual CLI and helpers it builds or imports. Read relevant upstream implementation and test bodies to understand changes. Reuse official upstream CI and source-test evidence for generic behavior. Do not inherit a previous upstream test cohort, expand execution from diff paths or function-name patterns, or rerun broad upstream suites as a daily gate.

Default execution covers affected Decodex-owned regressions against the exact prepared artifact, component/helper integrity and required repository checks. Run a small upstream test selection only to diagnose a concrete compatibility defect or establish specific missing proof for a retained fix. State the consumer and missing proof before that execution. An unconsumed upstream failure is a diagnostic limit, not a Decodex repair backlog item. Leave its cause unknown when unresolved; never count it as passed. Missing source review still blocks that commit's review cursor.

Use existing repository test entrypoints and already-managed tools. Follow repository version files and select tool paths only for the process. Use system-temporary homes with explicit CLI/helper paths; preserve host Cargo/Rustup locations as required by the fixture. Register exact tests first and check the executed count and unmatched selectors. Nextest can replace CARGO_BIN_EXE paths with its own source build outputs; those executions do not prove that the signed production artifact ran. Use the existing native fixture or direct compiled-test entrypoint when exact helper selection is required. Do not install or reconfigure host tools to repair fixture setup.

Keep test conversations in isolated Codex homes. If an explicitly authorized desktop acceptance check creates a conversation in the shared native home, archive it through the supported thread operation when finished. Do not call `thread/delete` there: another desktop client can retain a catalog entry after the rollout is removed. Archive retains the native history and supports restoration. Delete only a disposable isolated test home that this run owns.

Choose the narrowest repository-owned gate that covers the changed contract. Run applicable baseline checks and reproduce a defect before and after repair when practical. Run the final affected gate once after the code settles. Reuse evidence only for the same source, artifact, helpers, entrypoint, toolchain and feature configuration. Do not duplicate split check/lint/test runs after an aggregate already proves those contracts. Documentation or automation-control changes do not require app, provider or upstream source suites unless they affect those consumers. Preserve full failure output in the native run directory.

## Ownership and current state

Start from freshly fetched Decodex main in an isolated Codex-managed worktree. First reconcile prior pending PRs, exact-head CI, delivered or superseded changes, and related active work. Finish or repair an existing implementation batch before the next one. Do not overlap another maintainer or manual source catch-up owner. If ownership is uncertain, save a continuation without implementation or cursor writes. For another developer's active changes, record the affected item, owner and reason, then continue independent work. Do not message another chat without user authorization.

Keep the automation paused during an authorized manual source catch-up. Its owner must save the fixed target, certified cursor, unresolved dispositions, pending adaptations/PRs, optional decisions and verification limits before handoff. Reconcile that receipt with main. Resume only with explicit user authorization. A definition update does not itself authorize resumption.

The native automation directory's state.json owns the current review cursor and pending-work index. Memory, dated inventories, old continuations and archives are evidence, not current instructions. Keep certified ranges and their original coverage limits. Never advance a cursor because configuration, a baseline or another task's completion claim changed.

Use the sole checkpoint publisher:

    node --experimental-strip-types automations/upstream/checkpoint.mts --automation-dir <native-automation-directory>

This read-only form returns a compact current summary and state digest. Prepare a checkpoint input as specified in automations/upstream/README.md, then use the same command with --input <checkpoint.json>. Use --dry-run for a preview. Do not patch state.json, review counters or certificate indexes with ad hoc scripts. The publisher saves an immutable review receipt before atomically replacing the state. It derives cursor, certificate, next commit, remaining count and oldest unreviewed time together. It validates bookkeeping; it does not perform source review or runtime qualification. Ownership checks remain required; it is not a scheduler or distributed lock.

Use the same publisher for pending work, runtime and delivery observations. Keep source review, runtime qualification, PR delivery and installed-app acceptance separate. Reconcile finished historical blocks before explicitly archiving them. Preserve unresolved work in pending lists or followups and preserve certified ranges. Do not keep duplicate current counters or append new historical status blocks to the current state. A missing receipt is an evidence gap, not a regression or permission to move a cursor.

## Review a fixed upstream delta

Fetch official upstream main once and record its exact SHA and observation time as this run's fixed target. Generate a consecutive inventory after the certified cursor. Review it in bounded batches and continue through the fixed target within the same run. Keep screened count separate from completed review count.

For each reviewed commit, read the relevant diff, affected implementation and test bodies. Record changed behavior, exact source/test references and symbols, a concrete current Decodex consumer or exclusion evidence, tests read separately from executed identities, results and limits. Classify it as already covered, native-owned, not applicable, necessary repair, optional decision or deferred with a reason. Titles, path lists, file counts, generated schemas and ancestry are screening evidence only. For native-owned behavior, inspect the local integration boundary and whether the selected artifact contains the change.

If source, relevant tests or local impact remain unread or unresolved, keep that commit unreviewed and leave the contiguous cursor before it. Before a checkpoint, reconcile each completed record with its actual execution and delivery receipts; remove stale pending qualification claims. A reviewed range can still have pending adaptations or PRs. Keep them separate.

Read openwiki/decisions/upstream-product-scope.md and current consumers. Historical optional inventories do not preserve or restore retired features. Native Codex owns supported execution and enforcement; Decodex owns presentation and coordination. Keep one deduplicated proposal list with value, maintenance cost, evidence and decision status. Do not repeat unchanged, declined or delivered choices.

## Qualify the selected artifact

Development can follow a fixed reviewed official main commit before a Release. Freeze one commit per qualification batch. Use the explicit prepare-codex command to build or import its artifact. Verify actual source/archive provenance, CLI and helper identities, signatures, generated schema and affected native behavior. A commit string, matching schema or reused cache is not qualification. App startup and packaging must never compile Codex; development and release components share verification and cache repair.

Before a runtime migration, identify the merged Decodex fixes that must remain available from current main and retained contracts. Establish source inclusion or equivalent changes and run applicable existing Decodex native regressions against the exact candidate and helpers. Keep test-only changes, optional capabilities and runtime fixes distinct. If required proof is missing, keep the runtime change unmerged and preserve the current baseline, tests and pending fix records. Do not suppress regressions, relabel an older binary, lower the baseline, change runtime discovery or call an unsupported API.

Formal Decodex releases require an official Codex Release that contains the development baseline and passes the affected native regressions. Ancestry alone does not prove behavior. Development qualification does not authorize release, installation or publication.

## Evidence and bounded context

Keep the compact current evidence router in automations/upstream/retained-evidence.json. Stable consumer contracts, product decisions and original feature-qualification limits are in automations/upstream/retained-contracts.json. Update that catalog only when those requirements or their contract evidence change. Routine source review, selected artifact hashes, executed results and public code references belong in current-review.json and qualification.json; Git retains replaced versions. The index must identify the contract catalog, exact tests/selectors and entrypoints, commands, reusable fixture/source digests, actual artifact/helper and schema identities, qualified limits and public delivery evidence without a local build directory.

Keep private paths, project/chat IDs, account data, credentials, native definitions, full source trees, binaries and raw logs out of tracked files. Store detailed receipts in the native automation directory. Preserve required custom fixtures with their repository test owner. Missing local builds or logs do not stop independent review: reconstruct only affected proof from current source, tests and public delivery. A rebuilt artifact needs fresh identity and qualification; it is not the deleted original. Mark unrecovered historical claims unknown without erasing certified ranges or asking the user to restore obsolete builds.

Read and return bounded fields, relevant complete symbols and one OpenWiki page job at a time. Do not dump whole inventories, definitions, source suites or large tool responses into the conversation. When output truncates, narrow the next read instead of repeating the same dump. Keep full raw output native and return the result, counts, evidence location and material failure details.

Keep earlier coverage uncertainty in a separate baseline-audit queue. Check current consumers before reopening it. Do not repeat manual historical review, rescan old history or adopt old features to remove uncertainty. Require concrete missing proof or a reproducible retained-consumer defect before adding baseline work.

## Deliver one completed batch

Prepare code, reusable tests, portable evidence and relevant OpenWiki updates before the first push when practical. Use small signed local commits and a focused PR. Public code commit references are sufficient in tracked evidence; keep the PR number, live CI and merge receipt native. Do not create a commit solely to backfill a PR link. Attach each created PR to the chat and use codex plus codex-automation labels when available.

Use the supported OpenWiki page-job lifecycle. Keep stable behavior, ownership and procedures in Wiki. Prefer narrow source/test or stable-contract evidence; link current qualification instead of copying each run's counts, hashes and status into several pages. If unrelated stale work expands the job queue, record its incomplete lifecycle and owner instead of silently claiming completion or hand-editing generated state.

Wait for the final PR head's required checks. Fix real failures and rerun affected checks after material changes; do not reuse another head's CI as final proof. Merge with a clear imperative commit message, then verify remote main contains the delivered head and merge. Source review, executed tests, merge and installed acceptance are separate. Do not invent a human-acceptance gate from an older task.

Before cleanup, save the final checkpoint and native receipts. Remove only this run's unused branches, worktrees and temporary files after verifying ownership, delivered changes, clean state and no active use or dependencies. Preserve unfinished/unrelated work and host-managed processes. Never force cleanup.

Run once daily without a task-defined duration limit. Continue bounded batches until the fixed target and required repair, qualification, PR, CI and merge work are complete. End early only for a concrete blocker requiring user input or external change, with a precise continuation and pending CI head. Compare reviewed count, remaining count and oldest unreviewed time with prior completed receipts. If backlog grows across three consecutive completed runs, report it once with a bounded catch-up recommendation. Do not claim currentness or change the schedule automatically.

Notify once in Chinese after a material run. Use: 本轮状态 (已完成, 部分完成, 受阻); 已完成 (reviewed count/range, useful Decodex changes, merged PR links); 待继续 (remaining review, pending PR/CI, precise next action, or 无); 需你确认 (new or changed optional choices with value, cost and recommendation, or 无新增确认项); 验证范围 (executed Decodex checks and material consumer limits). Do not call required pending review or delivery complete. Report unconsumed upstream failures or unchanged inherited warnings only when they change a Decodex decision or require action; retain full diagnostics in evidence. Stay quiet on unchanged states and CI polls. Do not include private paths, account data or credentials.
