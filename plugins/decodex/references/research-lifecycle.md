# Decodex Research Lifecycle

Use this reference for the bounded research phase order and authority boundary.
Decodex is the default research surface for bounded Decodex technical investigation.

## Contract

Decodex research produces a latent `decodex.decision_contract/1` candidate. It does
not queue work, mutate Linear, set Codex goals, implement, or dispatch Program nodes.

The phase order is:

1. `research-probe`
2. `research-evidence`
3. `research-options`
4. `research-judgment`
5. `research-challenge`
6. `research-decision`
7. `research-promote` after explicit acceptance

The compact loop is: probe, evidence, options, judgment, challenge, decision.

## Probe

Record:

- decision question
- in-scope and out-of-scope surfaces
- success criteria and acceptance threshold
- constraints and non-goals
- stop rule or budget
- primary hypothesis
- rival hypotheses
- falsifiers
- first evidence plan
- expected promotion target or `no_promotion`

Do not gather broad evidence until the question and falsifiers can guide collection.

## Authority Boundary

Research remains latent until explicit acceptance such as "arrange this", "push this
forward", "推进", or "做". Promotion is a separate authority step.

Checked-in research belongs in `docs/research/` only as a flat JSON research artifact.
It is supporting knowledge, not implementation authority; runtime state may keep
structured Decision Contracts for machine use; checked-in research remains JSON and
non-authoritative until promoted.
