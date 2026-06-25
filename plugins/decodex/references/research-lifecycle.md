# Decodex Research Lifecycle

Use this for bounded Decodex research before execution. Research creates a latent
`decodex.decision_contract/1`; it does not queue work, mutate Linear, set Codex
goals, implement, or dispatch Program nodes.

## Loop

Probe, evidence, options, judgment, challenge, decision. Use `$deliberation:grill`
for unclear framing, `$deliberation:scout` for non-obvious evidence, and
`$deliberation:challenge` before material recommendations or `decision_ready`.
No evidence, no claim.

## Probe Contract

Record owner, question, scope, criteria, constraints, non-goals, expected output
shape, primary/rival hypotheses, falsifiers, evidence that would change the
recommendation, first evidence plan, stop rule, promotion target or `no_promotion`,
and smallest next check if evidence stays incomplete.

## Scale

- Light: local, low-ambiguity; inline framing plus compact challenge.
- Standard: scout only when evidence is not obvious; challenge still runs before
  recommendation or `decision_ready`.
- Deep: architecture, product, cross-boundary, root-cause, or public-contract;
  scout/challenge normally run as bounded read-only support agents.

Scale controls cost; the Decision Contract controls readiness.

## Delivery

Separate facts from inferences. Before terminal status, name owner, validation or
release check, rollback or freeze path, stop condition, and falsifier when execution
would follow.

Use source-backed docs, OKF, or repo-memory for non-trivial repo research. When a
recommendation changes skill/plugin behavior or comes from benchmarks, name the
benchmark/regression gate and falsifying result.

Research remains latent until explicit acceptance; promotion is separate. Checked-in
research belongs in `docs/research/` only as Markdown OKF research.
