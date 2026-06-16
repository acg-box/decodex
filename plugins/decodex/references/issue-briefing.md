# Decodex Issue Briefing Reference

Use this reference when accepted Decodex work needs normal Linear issue briefs,
existing issue-batch intake, or Program Intake readiness.

## Authority

Issue briefing is part of Decodex planning. It starts only after one of these
authority sources exists:

- an accepted and promoted Decision Contract
- an explicit execution instruction whose scope is already bounded
- a supplied batch of executable issue briefs for Program Intake

The briefing is not a separate delivery workflow. Do not route Decodex issue
briefing through an external delivery plugin, delivery handoff, or progress skill.
Runtime progress, review handoff, landing, closeout, and recovery remain
Decodex runtime surfaces.

## Generic Dispatch Briefing

Every Decodex-planned issue must be understandable by a generic implementation lane
without replaying the originating chat or reading private runtime state.

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

Use concrete repository paths, commands, specs, runbooks, and project policy only
when they exist. Do not invent modules, validation flows, tracker state, or runtime
authority to make an issue sound complete.

## Splitting Rules

Split accepted work only when one issue would be too broad for one retained lane.
Split by real ownership boundary, validation surface, dependency, or conflict
domain. Keep the issue set small enough that each issue can be started, reviewed,
and validated independently.

Each child issue must carry its own generic dispatch briefing. Name ordering only
when it is blocking. Do not expose internal graph ids, DAG edge editing, hidden goal
ids, or Program scheduler mechanics in the issue text.

## Existing Issue Intake

For `decodex intake issues`, treat the supplied issue descriptions as the public
briefing surface. If an issue is only a machine-readable block, private pointer, or
thin title, it is missing the generic dispatch briefing and should remain held until
the briefing is repaired.

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
