# Decodex Research Lifecycle

Use this for bounded Decodex research before execution. Research creates a latent
`decodex.decision_contract/1`; it does not queue work, mutate Linear, set Codex
goals, implement, or dispatch Program nodes.

## Loop

Probe, evidence, options, judgment, skeptic review, decision. Use
`$deliberation:grill` for unclear framing, `$deliberation:scout` for non-obvious
evidence, and `$deliberation:skeptic` before material recommendations or
`decision_ready`.
No evidence, no claim.

## Probe Contract

Record owner, question, scope, criteria, constraints, non-goals, expected output
shape, primary/rival hypotheses, falsifiers, evidence that would change the
recommendation, first evidence plan, stop rule, promotion target or `no_promotion`,
and smallest next check if evidence stays incomplete.

## Scale

- Light: local, low-ambiguity fact checks that end without a material recommendation;
  inline framing is acceptable.
- Standard: material research must use at least one bounded read-only support-agent
  scout or skeptic pass before a recommendation or `decision_ready` claim when
  support-agent tools are allowed.
- Deep: architecture, product, cross-boundary, root-cause, or public-contract
  research uses separate scout and skeptic support agents unless the user
  explicitly opts out or support-agent tooling is unavailable.

Scale controls cost; the Decision Contract controls readiness.
If support-agent tools are unavailable, name the inline fallback and do not describe
it as independent fresh-context review. Support agents stay read-only, receive one
bounded objective, and must not spawn further support agents unless the main thread
explicitly delegates that.

## Delivery

Separate facts from inferences. Before terminal status, name owner, validation or
release check, rollback or freeze path, stop condition, and falsifier when execution
would follow.

Use source-backed docs, OKF, or repo-memory for non-trivial repo research. When a
recommendation changes skill/plugin behavior or comes from benchmarks, name the
benchmark/regression gate and falsifying result.

Research remains latent until explicit acceptance; promotion is separate. Checked-in
research belongs in `docs/research/` only as Markdown OKF research.
