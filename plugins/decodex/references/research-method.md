# Decodex Research Method Reference

Use this reference when Decodex research needs the full Decision Contract protocol.
Decodex is the default research surface for bounded technical investigation.

## Contract-First Rule

The primary research output is a top-level `decodex.decision_contract/1` candidate.
The contract is a current-state snapshot of the decision, evidence, selected option,
gaps, validation expectations, and promotion target. It is not an append-only run log.

`docs/research/` is JSON-only, but the old `docs/research/*.json`
`research-run/2` event format is not the new Decodex research shape. The tracked
legacy JSON event logs were consolidated into
`docs/research/legacy-research-goal-audit.json`; use Git history only when raw old
event logs are needed as provenance. Do not write new Decodex research as old-shape
event logs, and do not infer current authority from an old research artifact.

## Loop

Run the same decision-quality loop for bounded research:

1. Probe: frame the decision, scope, success criteria, constraints, stop rule,
   hypotheses, rival hypotheses, and falsifiers.
2. Evidence: collect traceable observations, contradictions, inferences, source
   references, missing evidence, and private/public provenance.
3. Options: compare realistic choices, including status quo, minimal patch,
   architecture redesign, staged migration, and explicit no-go/defer when relevant.
4. Judgment: form one challenge-ready recommendation or an explicit non-decision.
5. Challenge: attack the judgment with skeptic objections.
6. Decision: finish as exactly one terminal status.
7. Promote: only after explicit acceptance.

The compact phase order is: probe, evidence, options, judgment, challenge, decision,
promote.

## Probe Checklist

Record these before broad search or implementation:

- decision question
- in-scope and out-of-scope surfaces
- success criteria and acceptance threshold
- constraints and non-goals
- stop rule or budget
- primary hypothesis
- realistic rival hypotheses
- falsifiers for the primary hypothesis
- initial evidence plan

The first durable event for machine-authored runs is `probe_completed`.

## Evidence Rules

- No evidence, no claim.
- Keep source or code references close to each claim.
- Separate observations, contradictions, inferences, and missing evidence.
- Prefer primary sources for code, APIs, specifications, policies, and live project
  state.
- Preserve conflicting evidence and name what would resolve it.
- Use private evidence refs for local, sensitive, or runtime-private proof.
- Map supported claims into `research_evidence`.

## Evidence Ledger Shape

Every Decision Contract candidate must carry an evidence ledger that can be read
without replaying the chat transcript.

Use these ledger classes:

| Class | Contract field | Use |
| --- | --- | --- |
| `external_source` | `research_provenance` or `research_evidence.kind/source_ref` | Public specifications, official docs, standards, changelogs, or vendor policy. Prefer exact URLs and version dates. |
| `repo_source` | `research_evidence.kind/source_ref` | Checked-in files, code references, fixtures, tests, docs, or command output from this repository. |
| `live_readback` | `research_evidence.kind` or `evidence_boundary.private_evidence_refs` | Current runtime, tracker, GitHub, Linear, local SQLite, or service state. Keep sensitive proof private. |
| `inference` | `research_evidence.kind/support` plus `accepted_authority.assumptions` when promoted | Reasoned conclusion derived from evidence. Name the evidence it depends on. |
| `gap` | `research_evidence.kind`, `execution_readiness.missing_decisions`, `risk_notes`, or `evidence_boundary` | Missing proof, unresolved contradiction, blocker, or human choice. |

Evidence is sufficient only when a later agent can answer these questions from the
contract:

- Which claims are supported by external evidence?
- Which claims are supported by repository state?
- Which claims are live readback rather than static docs?
- Which conclusions are inferences, not direct observations?
- Which gaps remain, and do they block `decision_ready`?

## Option Rules

For each option, record:

- what changes
- supporting evidence
- tradeoffs and risks
- what becomes easier or harder
- why the option is selected or rejected

Do not compare straw-man options or select an option without evidence or explicit
assumptions.
Map option records into `research_options`.

## Judgment Rules

A challenge-ready judgment includes:

- recommended option or explicit non-decision outcome
- why it best satisfies the decision criteria
- evidence ids, source references, or code references
- assumptions and constraints
- rejected alternatives
- unresolved evidence gaps
- expected validation if promoted

Do not call the judgment final before challenge.
For replayable machine-authored runs, assign a stable judgment id or hash over the
normalized conclusion and cited evidence.

## Challenge Rules

Challenge the judgment against:

- missing or contradictory evidence
- unexamined alternatives
- scope creep
- hidden operational cost
- compatibility or migration risk
- security, privacy, data, billing, or destructive-action risk
- authority mismatch
- validation gaps

Record each objection as resolved, unresolved, or out of scope. Unresolved material
objections block `decision_ready`.
Use a bounded skeptic worker only when it materially improves independence.

## Decision Statuses

Use exactly one terminal outcome:

- `decision_ready`: evidence, options, resolved challenge, objectives, validation,
  and proposed issue summaries are sufficient for issue shaping after promotion.
- `not_decision_ready`: useful evidence exists, but a decision would be unsafe or
  under-supported.
- `blocked`: research cannot proceed until a non-decision blocker is removed.
- `needs_human_decision`: remaining uncertainty is a human/product/authority choice.

Never use `decision_ready` because budget ended.
No unresolved decisions, evidence gaps, or blockers may remain for `decision_ready`.

## Decision Contract Shape

The durable output is a `decodex.decision_contract/1` payload retained in runtime
state. In chat or docs, present the same shape plainly.

Required sections:

- source intent and decision question
- terminal decision status
- evidence ledger and provenance
- realistic options and tradeoffs
- selected decision or why no safe decision exists
- assumptions, constraints, non-goals, objections, and stop conditions
- validation expectations
- promotion target, such as `docs/spec`, `docs/runbook`, `docs/reference`,
  `docs/decisions`, `plugins/decodex/skills`, runtime code, tests, or no promotion
- proposed issue summaries only when downstream work is appropriate
- unresolved decisions, evidence gaps, or blockers

Use the existing runtime fields this way:

| Need | Field |
| --- | --- |
| Original request and decision question | `source_intent` |
| External docs, repo files, legacy artifacts, or subwork used | `research_provenance` |
| Supported claims, evidence class, and source/code refs | `research_evidence` |
| Compared choices and selected/rejected rationale | `research_options` |
| Objectives, constraints, assumptions, objections, non-goals, and stop rules | `accepted_authority` |
| Readiness, validation, risks, issue summaries, promotion targets, conflict domains, and dispatch intent | `execution_readiness` |
| Acceptance metadata | `promotion` |
| Generated issues or Program nodes | `links` |
| Private proof and public-safe projections | `evidence_boundary` |

Do not bury the terminal status, selected option, or unresolved gaps only in an event
tail. They must be visible from the top-level contract.

## Promotion Boundary

Research output is latent. Promotion requires explicit acceptance or an equivalent
follow-up such as "arrange this", "push this forward", "推进", or "做".
Promotion is a separate authority step and the research-to-planning authority
boundary.

When promoting:

1. Identify the accepted contract.
2. Preserve its objectives, non-goals, constraints, assumptions, objections,
   validation expectations, proposed issue summaries, and stop conditions.
3. Refuse promotion when unresolved decisions, evidence gaps, or blockers remain.
4. Route accepted work to `planning`.
5. Let Program Intake persist Execution Program readiness and dispatch ready mapped
   nodes directly.

Promotion must choose the correct durable lane:

- `docs/spec/` when the accepted result defines correctness, schema, invariants, or
  required behavior.
- `docs/runbook/` when it defines an operator sequence.
- `docs/reference/` when it explains current implementation or repository structure.
- `docs/decisions/` when it records durable rationale or tradeoffs.
- `docs/research/` when the user explicitly asks for a supporting JSON research report
  or evidence extraction that should remain non-authoritative until promoted.
- `plugins/decodex/skills/` when it changes agent-facing workflow instructions.
- Runtime code and tests only when accepted behavior cannot be represented by docs or
  skills alone.

If the accepted contract needs multiple lanes, preserve one authority source and link
other lanes to it rather than duplicating the same rule.
