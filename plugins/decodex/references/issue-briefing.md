# Decodex Issue Briefing Reference

Use this when accepted Decodex work needs Linear issue briefs, issue-batch intake, or
Program Intake readiness.

## Authority

Issue briefing is planning-only and starts after one authority source:

- an accepted and promoted Decision Contract
- an explicit execution instruction whose scope is already bounded
- a supplied batch of executable issue briefs for Program Intake

The briefing is not delivery workflow. Do not route Decodex issue briefing through
an external delivery plugin, delivery handoff, or progress skill; runtime progress,
review handoff, landing, closeout, and recovery stay Decodex surfaces.

## Generic Dispatch Briefing

Every planned issue must work for a generic implementation lane without replaying chat
or private runtime state.

Include:

1. one outcome
2. required reading
3. in-scope work
4. explicit non-goals
5. current-tree landing zone
6. ownership boundary
7. acceptance criteria
8. validation expectations
9. real dependencies, blockers, or conflict domains
10. dispatch notes needed for a cold-start agent

Use real paths, commands, specs, runbooks, and policy. Do not invent modules,
validation, tracker state, or runtime authority.

## Splitting Rules

Split accepted work only when one issue is too broad for one lane, using real
ownership, validation, dependency, or conflict boundaries.

Each child issue must carry its own generic dispatch briefing. Name ordering only
when it is blocking. Do not expose internal graph ids, DAG edge editing, hidden goal
ids, or Program scheduler mechanics in the issue text.

## Existing Issue Intake

For `decodex intake issues`, issue descriptions are the public briefing surface. A
thin title, private pointer, runtime event, PR body, or checkpoint is not a generic
dispatch briefing and should remain held until repaired.

## Non-Goals

- Do not create a roadmap, ADR, or broad project plan.
- Do not promote latent research.
- Do not mutate Linear, apply queue labels, or persist Program Intake rows from the
  briefing step alone.
- Do not treat a briefing as proof that work is implemented, validated, reviewed,
  ready to land, or closed out.
