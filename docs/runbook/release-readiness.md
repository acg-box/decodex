---
type: "Runbook"
title: "Release Readiness"
description: "Procedure for checking Decodex release readiness before tagging or publishing."
status: active
authority: procedural
owner: automation
tags: [runbook]
last_verified: 2026-06-16
---
# Release Readiness

Goal: Execute the final Decodex v0.2.0 Loop Engineering release-candidate gate without
tagging or publishing before real dogfood and validation evidence exists.
Read this when: You are closing the v0.2.0 release gate, preparing tag readiness, or
checking whether the Loop Engineering candidate has enough evidence to publish.
Inputs: Current `main`, registered Decodex project config, registered project
`WORKFLOW.md`, external release automation, runtime evidence, PR handoff state, and
dogfood lane run id.
Depends on: `README.md`, `Makefile.toml`, `docs/spec/loop-runtime.md`,
`docs/spec/review-orchestration.md`,
`docs/reference/operator-control-plane.md`, and
`docs/runbook/review-config-migration.md`.
Outputs: A release-ready evidence packet, tag/version match for `v0.2.0`, and a final
release note that names shipped capabilities and deferred items.

## Tag Contract

- `Cargo.toml` `[workspace.package].version` must be `0.2.0`.
- The external release automation accepts only tags shaped as `vX.Y.Z`.
- The external release automation must fail when the requested tag does not equal
  `v${workspace.package.version}`.
- For this candidate, the only valid release tag is `v0.2.0`.
- Do not tag or publish if the release automation, workspace version, or intended tag
  disagree.

## Required Evidence

Collect evidence in this order:

1. Confirm the dependency chain is landed and the lane is based on current `main`.
   Required dependencies are XY-870, XY-871, XY-872, XY-873, XY-874, XY-875,
   XY-877, XY-878, and XY-890.
2. Confirm the operator-local self-project config uses:

   ```toml
   [codex]
   review = "standard"
   ```

3. Scan checked-in docs, skills, examples, and config templates for historical review
   config fields. Because this bullet names the removed keys to define the check,
   allow `internal_review_mode` and `external_review_enabled` in this bullet and in
   migration history only. They must not appear in active project configs, examples,
   templates, or other release procedures.
4. Run the registered project gate before any pushed PR head. This mirrors the
   registered `WORKFLOW.md` order: canonicalize first, then verify.

   ```sh
   cargo make fmt
   cargo make lint-fix
   cargo make test
   ```

5. Confirm canonicalization did not leave unreviewed changes. If `fmt` or `lint-fix`
   changed files, inspect the resulting diff before collecting release-gate evidence.
6. Run the full release gate on the final tree:

   ```sh
   cargo make check
   ```

7. Run focused loop, review, config, prompt, dry-run, and recovery checks selected
   from the landed dependency changes. At minimum include review-level and config
   coverage. Text search from step 3 is not sufficient: the release evidence must
   show the active project config parser rejects those removed fields, while the
   current `[codex].review` model remains covered by review/config tests.

   ```sh
   cargo test -p decodex review --all-features -- --test-threads=1
   cargo test -p decodex config --all-features -- --test-threads=1
   cargo test -p decodex loop_scenarios --all-features -- --test-threads=1
   cargo test -p decodex normal_prompts --all-features -- --test-threads=1
   cargo test -p decodex dry_run --all-features -- --test-threads=1
   cargo test -p decodex recovery --all-features -- --test-threads=1
   ```

8. Verify the current-source CLI path:

   ```sh
   decodex probe stdio://
   decodex status --live --json
   decodex run --dry-run
   ```

9. Dogfood at least one real Decodex lane with `[codex].review = "standard"`.
   The lane must reach PR handoff or another release-safe terminal state, and the
   runtime-owned review gate must leave an inspectable Decodex Review checkpoint.
   Manual PR review comments or local review notes do not replace the runtime
   checkpoint: `decodex evidence <ISSUE> --json` for the dogfood lane must show a
   non-empty `review_checkpoints` array. The release remains blocked, even if the
   implementation PR lands, when that runtime checkpoint is absent.
10. Read the dogfood lane private evidence:

   ```sh
   decodex evidence <ISSUE> --run-id <RUN_ID> --attempt <N> --json
   ```

   The readback must summarize review, recovery, boundary, or harness evidence when
   those events exist for the run.
11. Attach the concrete command outputs, dogfood issue, run id, attempt, PR URL or
    terminal state, and evidence readback summary to the release gate record before
    creating the `v0.2.0` tag.

## Release Note

### v0.2.0 Loop Engineering

Decodex v0.2.0 closes the Loop Engineering release candidate. The release moves
Decodex from isolated retained-lane automation toward an evidence-backed loop that can
connect accepted Decision Contracts, issue/program shaping, queued execution, review,
recovery, PR handoff, and release-readiness verification.

Shipped capabilities:

- Natural-language accepted-decision intake now compiles local Decision Contract
  candidates before Program readiness.
- Accepted Decision Contracts can feed internal Execution Program readiness while
  normal Linear issues remain the executable lane boundary.
- Phase-scoped Codex goals make implementation, validation repair, review repair, and
  handoff evidence distinct runtime phases.
- The review-level config model is consolidated under `[codex].review` with `off`,
  `standard`, and `strict` levels.
- Standard review requires Decodex Review through structured runtime-owned
  `issue_review_checkpoint` evidence after PR handoff and before clean-path landing.
- Loop guardrails, Architecture Recovery Packets, and Authority Boundary Checks
  preserve review, recovery, and human-required stop evidence instead of collapsing
  repeated failures into generic retry state.
- Operator status and `decodex evidence` can read compact loop status, review
  checkpoints, recovery summaries, boundary decisions, and harness-improvement
  candidates from local runtime evidence.
- The installable Decodex plugin, runbooks, and specs align with the Loop Engineering
  workflow and the `review = "standard"` dogfood path.

Intentionally deferred items:

- Do not expose Execution Program graph editing, DAG commands, or Codex goal internals
  as ordinary operator workflow.
- Do not use GitHub Review unless `[codex].review = "strict"` and the strict adapter
  signals are satisfied.
- Do not treat harness-improvement recommendations as automatic authority to edit
  prompts, validators, issue templates, skills, or policy.
- Do not tag or publish v0.2.0 until the evidence checklist above includes a real
  dogfood lane and a successful full release gate.
