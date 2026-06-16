# Spec Index

Purpose: Route agents to normative documents that define repository truth.

Question this index answers: "what must remain true?"

## Use this index when

- You need an invariant, contract, schema, enum, state model, interface, or required
  behavior.
- You are deciding whether code or data is correct.
- A runbook says "see the governing spec" and you need the authoritative source.

## Do not use this index when

- You need step-by-step instructions, maintenance actions, migrations, or incident
  response.
- You want rationale only, without an authoritative contract.
- You need current layout or implementation boundaries; read `docs/reference/index.md`.
- You need design rationale or packaging tradeoffs; read `docs/decisions/index.md`.

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
- Choose names for topic clarity and retrieval quality, not visual uniformity.
- If a runbook depends on a spec, the runbook links back to the governing spec.

## Current governing specs

- [`loop-runtime.md`](./loop-runtime.md) defines the natural-language-first loop
  runtime, Decodex-native Research/Decision stage, latent Loop/Decision Contract,
  internal Execution Program, phase-scoped goals, unattended execution behavior, and
  loop guardrails.
- [`runtime.md`](./runtime.md) defines the runtime state model, reconciliation rules, and
  tracker writeback boundaries.
- [`app-server.md`](./app-server.md) defines the direct Codex `app-server` interaction
  contract and protocol support evidence used by the runtime.
- [`lane-control.md`](./lane-control.md) defines the CLI/API-first operator
  lane-control capability matrix and the boundary between bottom-layer steer support
  and higher-level policy guardrails.
- [`lane-control-state.md`](./lane-control-state.md) defines the authoritative lane
  control state axes, invariants, guard semantics, terminal barrier, and projection
  rules used by scheduler decisions and operator status.
- [`github-change-bundle.md`](./github-change-bundle.md) defines the normalized GitHub
  input model for PR-first public signal analysis.
- [`signal-entry.md`](./signal-entry.md) defines the published signal-entry schema used
  by the static site.
- [`release-delta.md`](./release-delta.md) defines the stable-versus-prerelease summary
  artifact used by the homepage release-delta module.
- [`radar-ledger.md`](./radar-ledger.md) defines the local SQLite ledger that keeps
  every observed upstream Codex commit traceable without storing all raw history in Git.
- [`upstream-review.md`](./upstream-review.md) defines the deterministic upstream review
  queue and AI review boundary for every observed upstream Codex commit or PR.
- [`radar-artifact-retention.md`](./radar-artifact-retention.md) defines the 21-day Git
  hot window for raw Radar artifacts, the warm curated artifacts that stay in Git, and
  the GitHub Release archive manifest contract.
- [`upstream-impact.md`](./upstream-impact.md) defines how Radar classifies upstream
  Codex changes for public signals, Control Plane follow-up, and Publisher angles.
- [`social-candidate.md`](./social-candidate.md) defines source-backed public Publisher
  handoff artifacts that are not publication records.
- [`social-publishing.md`](./social-publishing.md) defines the checked-in social
  publication, block, skip, and failure records for `@decodexspace`.
- [`site-contract.md`](./site-contract.md) defines the static-site page budget,
  homepage obligations, and card rendering contract.
- [`linear-execution-ledger.md`](./linear-execution-ledger.md) defines the versioned
  Linear comment event-ledger schema for low-frequency Decodex lane transitions.
- [`agent-evidence.md`](./agent-evidence.md) defines the local agent-readable evidence
  files written under `~/.codex/decodex/agent-evidence/<service-id>/`.
- [`commit-messages.md`](./commit-messages.md) defines the machine-readable commit-message
  contract for local history.
- [`installable-agent-policy.md`](./installable-agent-policy.md) defines the boundary
  between portable installable `AGENTS.md` guidance and Decodex-specific repository,
  runtime, workflow, identity, and lifecycle policy.
- [`owned-lane-policy.md`](./owned-lane-policy.md) defines the fallback policy for
  Decodex-owned lanes, including manual-intervention and automatic-recovery decisions.
- [`review-orchestration.md`](./review-orchestration.md) defines the shared
  Self Check, Decodex Review, and GitHub Review loop, strict GitHub Review signals, round accounting, and
  direct landing entry rules.
- [`post-review-lifecycle.md`](./post-review-lifecycle.md) defines the normative post-
  `In Review` lane phases, transitions, and ownership boundaries through landing,
  closeout, and cleanup.
- [`tracker-tools.md`](./tracker-tools.md) defines the issue-scoped tracker write
  contract.
- [`workflow-file.md`](./workflow-file.md) defines registered project `WORKFLOW.md`
  configuration semantics and required fields.
