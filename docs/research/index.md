# Research Index

Purpose: Route agents to Markdown-only active or superseded research concepts.

Research output is latent until accepted and promoted. Research concepts preserve
evidence, options, challenge notes, terminal status, promotion targets, drift impact,
and disposition. They do not authorize implementation or own current truth.

## Concepts

- [`mcp-remote-control-productization.md`](mcp-remote-control-productization.md) is
  active research on the remaining MCP remote-access questions after bearer auth and
  process smoke promotion: OAuth Protected Resource Metadata, operator-loop-hosted
  scan, and future protocol compatibility.
- [`research-runtime-boundary.md`](research-runtime-boundary.md) is superseded
  provenance for the research runtime-boundary investigation. Current guidance lives
  in [`../decisions/okf-research-knowledge-lifecycle.md`](../decisions/okf-research-knowledge-lifecycle.md)
  and [`../reference/research-concepts.md`](../reference/research-concepts.md).

## Maintenance

- New research concepts must follow [`../policy.md`](../policy.md).
- Do not add non-Markdown artifacts under `docs/research/`.
- Promote accepted research into the correct OKF owners: `docs/decisions/` for
  rationale, `docs/spec/` for normative truth, `docs/reference/` for current state,
  `docs/runbook/` for procedures, and `docs/evidence/` for reusable proof.
- End each completed research concept as `continue`, `promote_and_supersede`,
  `promote_and_retire`, or `reject_or_deprecate`, then record the maintenance event
  in [`../log.md`](../log.md).
- Keep superseded research out of active LLM Wiki routing except as explicit
  provenance pointing to authoritative owners.
- When accepted research changes agent-facing workflow instructions, update the
  matching `plugins/decodex/skills/` files alongside the promoted docs concept.
