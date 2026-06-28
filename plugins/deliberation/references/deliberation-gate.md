# Deliberation Gate

Use this compact gate for design, architecture, refactor, root-cause debugging,
repeated failed fixes, bounded research, option comparison, and important ready/done
claims.

The gate is a method cue, not a ceremony:

1. Grill: identify the goal, real constraints, non-goals, smallest viable path, and
   falsifier.
2. Scout: use direct evidence from the repo, docs, OKF/LLM Wiki, code, commands, or
   runtime readback when facts are not local and obvious.
3. Skeptic: test the claim for objections, missing evidence, hidden assumptions,
   stale readbacks, and smaller alternatives before material conclusions.

Inline exception: inline deliberation is allowed only when one explicit local
question can be answered from 1-2 files or one command, and the answer cannot affect
architecture, review repair, root-cause debugging, public contracts, docs drift,
commit/land, or ready/done claims.

When the inline exception does not apply and subagent tools are allowed, use a
fresh bounded read-only subagent for scout or skeptic work. The main thread
keeps implementation ownership, checks evidence, and owns final claims. Do not wait
for the user to explicitly request subagents when the gate requires independent
evidence or critique.

Subagents receive one bounded objective, allowed roots or sources, excluded
surfaces, and an expected output shape. They stay read-only and must not spawn further
subagents unless the main thread explicitly delegates that. If subagent
tools are unavailable, name the inline fallback and do not describe it as independent
fresh-context review.
