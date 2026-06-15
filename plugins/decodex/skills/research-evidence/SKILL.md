---
name: research-evidence
description: Use during Decodex research evidence collection. Builds an auditable ledger of observations, sources, contradictions, inferences, missing evidence, and source-family coverage.
---

# Decodex Research Evidence

## Goal

Make every research claim traceable. Evidence is retained as context for a latent
Decision Contract, not as execution authority by itself.

## Evidence Rules

- No evidence, no claim.
- Give evidence items stable ids when the run is more than a short chat answer.
- Record source or code references close to the claim.
- Separate observation, contradiction, inference, and missing evidence.
- For external research, record source family or perspective when coverage breadth
  matters.
- Prefer primary sources for code, APIs, specifications, policies, and live project
  state.
- When evidence conflicts, preserve the contradiction and decide what would resolve it.

## Decodex Mapping

Map evidence into the Decision Contract as:

- `research_provenance` for source families, inspected files, commands, run outputs, or
  prior artifacts
- `research_evidence` for supported claims
- `evidence_gaps` for missing proof that blocks decision-ready status
- `private_evidence_refs` when the proof is local, sensitive, or runtime-private
- `public_projection_refs` only for sparse public-safe references

## Boundaries

- Do not use a research summary as authority to execute. It remains latent until
  promotion.
- Do not mirror private runtime payloads into public issue text.
- Do not flatten contradictions into a single confident claim unless the resolution is
  explicitly evidenced.
