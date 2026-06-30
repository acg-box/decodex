---
type: Drift Audit
title: Docs Self-Iteration
description: Audits the OKF docs maintenance loop against CLI and repository gate evidence.
status: active
authority: evidence
owner: docs
tags: [semantic-drift, docs, okf]
source_refs: []
code_refs: [apps/decodex/src/docs_okf.rs, apps/decodex/src/cli.rs, apps/decodex/src/agent/tracker_tool_bridge/tools.rs, apps/decodex/src/orchestrator/prompting.rs, apps/decodex/src/orchestrator/execution.rs, Makefile.toml]
related: [../policy.md, ../log.md]
drift_watch: [decodex docs check, decodex okf init, decodex okf check, issue_progress_checkpoint, issue_terminal_finalize, docs_impact, code_refs, related, promotes_to, drift_watch]
last_verified: 2026-06-30
---

# Docs Self-Iteration

## Watched Claims

- `docs/` is a Markdown-only OKF bundle.
- `decodex docs check` validates required routing files, Markdown-only artifacts,
  typed concept frontmatter, required research/drift headings, local Markdown links,
  structured frontmatter refs, and at least one drift audit evidence anchor.
- `decodex docs check` is the only supported docs validation subcommand; command
  aliases are not allowed.
- `decodex okf init` scaffolds portable `core`, `wiki`, and `repo-memory` bundles,
  refuses divergent overwrites, and validates the generated bundle before returning
  success.
- `decodex okf check` validates portable bundles by profile: `core`, `wiki`,
  `repo-memory`, or `decodex`.
- Structured OKF frontmatter refs are checked when present: `source_refs`,
  `code_refs`, `related`, `promotes_to`, `drift_watch`, and `tags`.
- Decodex records docs impact as private `docs_impact` evidence on
  `issue_progress_checkpoint` before every terminal finalize path, including manual
  attention, and the latest checkpoint must match the current lane `HEAD`.
- `cargo make check` includes the docs gate.

## Evidence Anchors

- `apps/decodex/src/docs_okf.rs`
- `apps/decodex/src/cli.rs`
- `apps/decodex/src/agent/tracker_tool_bridge.rs`
- `apps/decodex/src/agent/tracker_tool_bridge/tools.rs`
- `Makefile.toml`

## Reverse Checks

- Search for non-Markdown artifacts under `docs/`.
- Search for stale references to JSON-only research docs.
- Search for command aliases or stale `decodex docs lint` references.
- Run `decodex docs check`.
- Run `decodex okf init` against a temporary bundle and verify the generated bundle
  passes the selected profile.
- Verify terminal finalize rejects terminal paths without a current-HEAD docs-impact
  checkpoint.

## Verdict

pass

The OKF gate exists, all checked-in docs concepts have typed frontmatter and required
contract headings, stale JSON-only research docs claims have been replaced with
Markdown OKF research concept rules, `decodex docs check` passes for the repository
docs bundle, the docs validation command has no alias, structured frontmatter refs are
validated when present, portable
`decodex okf init` and `decodex okf check` profiles are available for non-Decodex
bundles, and Decodex terminal paths require the latest private docs-impact checkpoint
to match the current lane `HEAD`.

## Required Updates

- Continue adding narrower drift audit evidence concepts when future lanes change
  command behavior, status fields, validation, workflow, schemas, or operator
  procedures.

## Citations

- [`../policy.md`](../policy.md)
- [`../log.md`](../log.md)
