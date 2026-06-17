# Decodex Issue Briefing Reference

Use this when accepted Decodex work needs Linear issue briefs, issue-batch intake, or
Program Intake readiness.

## Authority

Issue briefing is part of planning. It starts only after one authority source exists:

- an accepted and promoted Decision Contract
- an explicit execution instruction whose scope is already bounded
- a supplied batch of executable issue briefs for Program Intake

The briefing is not a delivery workflow. Do not route Decodex issue briefing through
an external delivery plugin, delivery handoff, or progress skill. Runtime progress,
review handoff, landing, closeout, and recovery stay Decodex runtime surfaces.

## Generic Dispatch Briefing

Every Decodex-planned issue must be understandable by a generic implementation lane
without replaying chat or private runtime state.

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

Split accepted work only when one issue is too broad for one lane. Split by real
ownership boundary, validation surface, dependency, or conflict domain.

Each child issue must carry its own generic dispatch briefing. Name ordering only
when it is blocking. Do not expose internal graph ids, DAG edge editing, hidden goal
ids, or Program scheduler mechanics in the issue text.

## Existing Issue Intake

For `decodex intake issues`, issue descriptions are the public briefing surface. A
machine-readable block, private pointer, or thin title is missing the generic
dispatch briefing and should remain held until repaired.

Do not use a progress checkpoint, review summary, PR body, or runtime event as a
substitute for the issue briefing. Those surfaces can support evidence, but the
normal issue remains the executable lane boundary.

## Non-Goals

- Do not create a roadmap, ADR, or broad project plan.
- Do not promote latent research.
- Do not mutate Linear, apply queue labels, or persist Program Intake rows from the
  briefing step alone.
- Do not treat a briefing as proof that work is implemented, validated, reviewed,
  ready to land, or closed out.
