# Spec Index

Purpose: Route agents to normative documents that define repository truth.

Question this index answers: "what must remain true?"

## Use this index when

- You need an invariant, contract, schema, enum, state model, interface, or required
  behavior.
- You are deciding whether code or data is correct.
- A guide says "see the governing spec" and you need the authoritative source.

## Do not use this index when

- You need step-by-step instructions, maintenance actions, migrations, or incident
  response.
- You need a planning-tool artifact or a saved execution plan under `docs/plans/`.
- You want rationale only, without an authoritative contract.

## What belongs in `docs/spec/`

- Contracts and invariants.
- Data shapes, canonical field names, enums, defaults, units, and limits.
- State transitions and protocol rules.
- Behavior that tests, code, or operators should treat as authoritative.

## Spec document contract

Start each spec with a compact routing header:

- `Purpose`
- `Status: normative`
- `Read this when`
- `Not this document`
- `Defines`

Then keep the body explicit:

- Prefer concrete nouns over pronouns.
- Separate facts from rationale.
- Include canonical names exactly as code or data uses them.
- Include a small example when it removes ambiguity.
- Link to related guides instead of embedding procedures.

## Structure policy

- Prefer shallow paths while the spec set is small.
- Add subfolders only when they mirror stable system boundaries or materially reduce
  ambiguity.
- Do not require fixed filename prefixes up front.
- Choose names for topic clarity and retrieval quality, not visual uniformity.
- If a guide depends on a spec, the guide links back to the governing spec.

## Current specs

- `repo_layout.md` defines the canonical top-level layout during the transition away
  from the Rust-only template surface.
- `site_contract.md` defines the MVP page budget, homepage obligations, and card
  rendering contract.
- `github_change_bundle.md` defines the normalized GitHub input model for PR-first
  analysis.
- `signal_entry.md` defines the published signal-entry schema used by the site.
- `release_delta.md` defines the stable-versus-prerelease summary artifact used by
  the homepage release-delta module.
