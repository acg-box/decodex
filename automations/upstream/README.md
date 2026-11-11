# Codex upstream synchronization

The Codex app's standalone cron automation, `codex-upstream-maintainer`, owns
incremental source review, retained-capability repairs, qualification and
delivery. Each daily occurrence starts a new chat in an assigned managed
worktree. The [duty definition](duty.json) declares the model, effort, execution
mode and daily UTC cadence. The [maintainer prompt](prompts/maintainer.md) owns
the workload contract. `automations/portfolio.toml` declares the app automation;
a repository declaration alone does not activate it. Archiving a completed run
chat does not retire the standalone automation.

The former Decodex Agent wake, `codex-upstream-maintenance-daily`, is cancelled.
Keep it cancelled while the Codex app owns this workload. Its same-chat scheduler
acceptance remains historical evidence and does not prove fresh-chat execution.

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

The existing maintenance directory under the local Decodex application data
root retains the shared `state.json` cursor and pending index across fresh chats.
Its historical internal handoff records that location and previous evidence
directories, not current scheduler ownership. Keep these accepted state and
evidence locations; do not start a second cursor in the app automation directory.
Memory, dated receipts and archives remain evidence. Establish one owner
before writing; the checkpoint command is not a scheduler or distributed lock.
Use the already-managed Node version from `site/.nvmrc`:

```sh
node --experimental-strip-types automations/upstream/checkpoint.mts \
  --automation-dir "$DUTY_STATE_DIRECTORY"
node --experimental-strip-types automations/upstream/checkpoint.mts \
  --automation-dir "$DUTY_STATE_DIRECTORY" --input "$RUN_DIRECTORY/checkpoint.json" --dry-run
node --experimental-strip-types automations/upstream/checkpoint.mts \
  --automation-dir "$DUTY_STATE_DIRECTORY" --input "$RUN_DIRECTORY/checkpoint.json"
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
together. The user selected standalone Codex app runs; keep the former internal
wake cancelled. Resume a manually paused app automation only under explicit user
authorization.

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
