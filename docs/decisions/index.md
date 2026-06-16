# Decisions Index

Purpose: Route agents to durable design choices that explain why the repository is shaped
this way.

Question this index answers: "why was it designed this way?"

## Use this index when

- You need the rationale behind a stable repository or packaging choice.
- You need to understand tradeoffs that should survive implementation churn.
- You are considering changing an existing design boundary and need the prior reasoning
  first.

## Do not use this index when

- You need the current operator sequence.
- You need the current implementation map only.
- You need the normative contract without the rationale layer.

## Current decisions

- [`natural-language-loop-runtime.md`](./natural-language-loop-runtime.md) records why
  Decodex keeps execution-graph semantics internal behind a natural-language research
  and promotion surface.
- [`decodex-plugin-source.md`](./decodex-plugin-source.md) records why this repository
  owns the canonical Decodex plugin and why generic Playbook guidance should only keep
  portable routing.
- [`mcp-capability-gateway-and-skill-slimming.md`](./mcp-capability-gateway-and-skill-slimming.md)
  records why Decodex should introduce an MCP capability gateway while slimming
  skills into static routing, authority, and safety entrypoints.
- [`radar-control-plane-publisher.md`](./radar-control-plane-publisher.md) records the
  stable capability names for upstream Codex intelligence, retained-lane orchestration,
  and public publishing after the repository integration.
- [`codex-upstream-radar-redesign.md`](./codex-upstream-radar-redesign.md) records why
  continuous upstream Codex tracking now starts from deterministic review queues and
  keeps AI judgment in Codex automation rather than GitHub Actions.
- [`radar-artifact-release-archives.md`](./radar-artifact-release-archives.md) records
  why old raw Radar bundles and analysis drafts leave Git after 21 days and move to
  dedicated GitHub Release assets with checked-in manifests.
- [`static-public-site.md`](./static-public-site.md) records why the public Decodex site
  remains static while runtime/operator behavior stays in the CLI and local control
  plane.
