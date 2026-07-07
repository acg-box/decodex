# Reference Index

Purpose: Route agents to descriptive documents that explain the repository's current
structure and implementation surfaces.

Question this index answers: "how is it currently organized or implemented?"

## Use this index when

- You need the current repository layout, ownership boundaries, or where a topic lives.
- You need to know which directory or file surface is authoritative for a class of work.
- You need to understand where Decodex-specific evidence and runtime references fit.

## Do not use this index when

- You need a normative contract.
- You need an execution sequence or operator runbook.
- You need durable rationale for why a design choice exists.

## Current reference docs

- [`build-test-run.md`](./build-test-run.md) for repo-native setup, build, test, run,
  validation, task-runner automation, and source-entrypoint commands.
- [`codex-compatibility-matrix.md`](./codex-compatibility-matrix.md) for current
  source-backed Decodex compatibility evidence against upstream Codex stable and
  preview CLI/app-server releases.
- [`operator-control-plane.md`](./operator-control-plane.md) for the current
  single-machine control-plane shape, operator dashboard sections, local-vs-external
  state boundary, and deferred operator directions.
- [`github-operations.md`](./github-operations.md) for the current keep-vs-replace map
  for `gh`-backed GitHub operations, custom `gh api`/GraphQL reads, and local Git
  cleanup boundaries.
- [`test-suite.md`](./test-suite.md) for the current test inventory, behavior grouping,
  and keep/merge/delete standards.
- [`workspace-layout.md`](./workspace-layout.md) for the repository surface map and
  directory ownership boundaries, including the canonical Decodex plugin source.
