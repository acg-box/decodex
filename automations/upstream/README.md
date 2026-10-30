# Codex upstream synchronization

One daily task, `codex-upstream-maintainer`, owns incremental source review,
retained-capability repairs, qualification and delivery. Its desired model,
effort, worktree environment and daily UTC cadence are in
`automations/portfolio.toml`. The [maintainer prompt](prompts/maintainer.md) owns
the workload contract. Native tools own registration; repository declarations do
not activate or resume a task.

Upstream owns generic Codex correctness. Read its relevant implementation and
test bodies, and reuse upstream CI evidence. Default execution covers affected
Decodex-owned consumers against the exact prepared CLI/helpers and the required
repository gate. Run upstream tests only for a concrete compatibility diagnosis
or specific missing proof. Do not inherit old source-suite selections. Keep an
unconsumed upstream failure as a diagnostic limit, not a Decodex repair item.

Development can use a fixed reviewed official main commit before a Release.
Qualify the actual artifact, helpers, provenance, schema and retained fixes with
explicit `prepare-codex`. Formal releases require a qualified official Release
that retains the development baseline. App startup and packaging never compile
Codex. Installation, startup, tags and publication are outside maintenance scope.

## Current state and checkpoint

The native automation directory's `state.json` is the current cursor and pending
index. Memory, dated receipts and archives remain evidence. Establish one owner
before writing; the checkpoint command is not a scheduler or distributed lock.
Use the already-managed Node version from `site/.nvmrc`:

```sh
node --experimental-strip-types automations/upstream/checkpoint.mts \
  --automation-dir "$AUTOMATION_DIRECTORY"
node --experimental-strip-types automations/upstream/checkpoint.mts \
  --automation-dir "$AUTOMATION_DIRECTORY" --input "$RUN_DIRECTORY/checkpoint.json" --dry-run
node --experimental-strip-types automations/upstream/checkpoint.mts \
  --automation-dir "$AUTOMATION_DIRECTORY" --input "$RUN_DIRECTORY/checkpoint.json"
```

The read-only form returns a compact summary and state digest. The publisher
saves an immutable review receipt before atomically replacing the state. It
updates cursor, certificate, remaining count and next commit together, rejects
stale/incomplete input and supports exact replay. Use it for pending work and
runtime/delivery observations too; do not patch counters with per-run scripts.
It validates bookkeeping, not the truth of source or runtime qualification.
See the [checkpoint input contract](../../openwiki/operations/codex-upstream-autopilot.md#publish-a-checkpoint)
for input fields, explicit historical-field archival and recovery limits.

After manual delivery, reconcile later merges and their native receipts with main.
Replace the current continuation with one accurate next action. Keep deferred
issues separate from approved implementation and daily source-review backlog.
Save the fixed target, certified prefix, remaining work and verification limits
together. Keep the schedule paused until the user explicitly requests resumption.

## Portable evidence and delivery

[retained-evidence.json](retained-evidence.json) is the compact current router.
It binds [retained-contracts.json](retained-contracts.json), which owns stable
consumer requirements, selected product decisions and original feature limits.
Change that catalog only when its contracts or evidence change. Routine source
review and actual artifact/test results stay in `current-review.json` and
`qualification.json`; Git retains replaced versions.

Wiki owns stable behavior, responsibility and procedures. Prefer narrow source,
test or stable-contract Claim evidence. Link current qualification instead of
copying each run's counters/hashes into several pages. Use its supported lifecycle.
Keep raw logs, private paths, account data, native definitions and builds native.
Missing local artifacts require only affected proof to be rebuilt and qualified;
a rebuilt artifact is not the deleted original.

Complete portable evidence edits and verify the contract-catalog digest before
starting Wiki generation. Freeze those inputs for the page jobs. PR, CI and merge
observations remain native and do not require an evidence-only follow-up commit.

Finish code, tests, evidence and related Wiki jobs before the first push when
practical. Use known code commits for portable public references; keep PR numbers,
live CI and merge receipts native. Avoid a commit solely to backfill a PR link.
Require final-head CI and verify remote main after merge. Notify only material
Decodex effects, decisions and blockers. Preserve unrelated resources on cleanup.

Validation:

```sh
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo +stable make test-automations
```

The automation gate includes the checkpoint regressions. Workflow-only changes
do not require building Codex, app/provider tests or broad upstream suites.
The runtime evaluator reports drift; it does not change native definitions.
