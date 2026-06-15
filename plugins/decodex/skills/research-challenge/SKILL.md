---
name: research-challenge
description: Use before finalizing Decodex research to run skeptic objections against the current judgment. Unresolved objections block decision-ready status.
---

# Decodex Research Challenge

## Goal

Make the recommendation survive adversarial review before it can become
decision-ready.

## Challenge Pass

Challenge the current judgment against:

- missing evidence
- contradictory evidence
- unexamined alternatives
- scope creep
- hidden operational cost
- compatibility or migration risk
- security, privacy, data, billing, or destructive-action risk
- authority mismatch with the user's request or accepted Decision Contract
- validation gaps

Record each objection as resolved, unresolved, or out of scope. Resolved objections
should explain what evidence or constraint resolves them. Unresolved objections become
`unresolved_decisions`, `evidence_gaps`, `risk_notes`, or `blockers`.

## Worker Use

Use a bounded skeptic worker only when it materially improves independence. The skeptic
must not edit files, mutate tracker state, promote contracts, dispatch work, or recurse
into further workers.

## Boundaries

- Do not finalize `decision_ready` while material objections remain unresolved.
- Do not downgrade real blockers into cosmetic risk notes.
- Do not let challenge create execution authority; it only decides whether the latent
  contract can be safely promoted later.
