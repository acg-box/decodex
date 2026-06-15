# Decodex Research Method Reference

Use this reference when Decodex research needs the full Decision Contract protocol.
Decodex is the default research surface for bounded technical investigation.

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
state. In chat, present the same shape plainly:

- source intent and decision question
- evidence and provenance
- realistic options and tradeoffs
- selected decision or why no safe decision exists
- assumptions, constraints, non-goals, objections, and stop conditions
- validation expectations
- proposed issue summaries only when downstream work is appropriate
- unresolved decisions, evidence gaps, or blockers

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
