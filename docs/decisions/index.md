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
  owns the canonical Decodex plugin, including reusable repo-work guidance.
- [`mcp-capability-gateway-and-skill-slimming.md`](./mcp-capability-gateway-and-skill-slimming.md)
  records why Decodex should introduce an MCP capability gateway while slimming
  skills into static routing, authority, and safety entrypoints.
- [`okf-research-knowledge-lifecycle.md`](./okf-research-knowledge-lifecycle.md)
  records why research promotion is an OKF knowledge operation and how LLM Wiki
  hygiene keeps `docs/research/` from competing with authoritative owners.
- [`static-public-site.md`](./static-public-site.md) records why the public Decodex site
  remains static while runtime/operator behavior stays in the CLI and local control
  plane.
