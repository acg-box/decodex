---
name: research
description: Use when Decodex should run bounded evidence-based research or design investigation before execution. Owns the Decodex-native probe, evidence, options, judgment, challenge, decision, and promotion method through latent Decision Contracts.
---

# Decodex Research

## Goal

Make Decodex the default research surface for bounded technical investigation. A
research request produces a latent Decision Contract candidate. It does not create
execution authority until the user or accepted runtime policy promotes it.

Use this skill when the user says `research X`, asks for a design investigation, asks
whether a plan is the best architecture, or wants a decision-ready answer before
implementation.

## Method

Run the same decision-quality loop every time:

1. Use `research-probe` to frame the decision, scope, success criteria, constraints,
   stop rule, primary hypotheses, rival hypotheses, and falsifiers before broad
   evidence collection.
2. Use `research-evidence` to collect an auditable evidence ledger. No evidence, no
   claim. Separate observations, contradictions, inferences, and missing evidence.
3. Use `research-options` to compare realistic options, including the status quo when
   relevant. Tie tradeoffs back to evidence ids or explicit assumptions.
4. Use `research-judgment` to form one challenge-ready recommendation or to state why
   the run is not decision-ready.
5. Use `research-challenge` to attack the judgment with skeptic objections. Unresolved
   objections block `decision_ready`.
6. Use `research-decision` to finish as `decision_ready`, `not_decision_ready`,
   `blocked`, or `needs_human_decision`.
7. Use `research-promote` only after explicit acceptance or an equivalent follow-up
   such as `arrange this`, `push this forward`, `推进`, or `做`.

## Decision Contract Output

The durable Decodex output is a `decodex.decision_contract/1` payload retained in
runtime state. In chat, present the same shape plainly:

- source intent and decision question
- evidence and provenance
- realistic options and tradeoffs
- selected decision or why no safe decision exists
- assumptions, constraints, non-goals, objections, and stop conditions
- validation expectations
- proposed issue summaries only when downstream work is appropriate
- unresolved decisions, evidence gaps, or blockers

`decision_ready` is allowed only when the result has enough evidence, option
comparison, resolved challenge, accepted objective, validation expectations, and
proposed issue summaries for downstream issue shaping after promotion. It is still
latent until promoted.

## Boundaries

- Do not route Decodex research through the legacy external `$research` skill as the
  primary method.
- Do not treat `docs/research/` artifacts as current authority. They are legacy or
  supporting evidence that can be cited or imported into a Decision Contract.
- Do not queue work, mutate Linear, set Codex goals, start implementation, or dispatch
  Program nodes from research alone.
- Do not hide missing evidence. Return `not_decision_ready`, `blocked`, or
  `needs_human_decision` instead of forcing a recommendation.
- Use subagents only for bounded scout, analyst, or skeptic support. The main agent
  owns the coherent Decision Contract and final decision status.
