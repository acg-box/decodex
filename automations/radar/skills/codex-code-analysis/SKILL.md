---
name: codex-code-analysis
description: Use when reading official Codex source, PR, commit, schema, or test evidence to explain a change and its possible Decodex impact.
---

# Codex Code Analysis

Use this repository-local skill to produce a defensible interpretation of one
upstream Codex change. It is an optional research aid, not workflow authority.

## Inputs

Use the best available official evidence. This can be a GitHub PR, commit, source
file, protocol schema, test, release note, or a validated Radar bundle. Do not
require a Radar artifact when direct official evidence is clearer.

## Analysis

1. Identify the changed behavior and the exact source anchor.
2. Follow enough of the runtime path to distinguish shipped behavior from
   plumbing, groundwork, tests, documentation, or cleanup.
3. Explain what a Codex user or operator can observe.
4. Compare the behavior with current Decodex code and tests.
5. Separate required compatibility work, useful feature adoption, editorial
   value, and no-change outcomes.
6. State confidence and the evidence that would falsify the conclusion.

Prefer protocol or schema changes, executable behavior, direct tests, and
official documentation. A title, file name, social post, TODO, or sparse release
note is not sufficient by itself.

## Output

Return a short analysis with:

- observed change;
- official source URLs and concrete anchors;
- user or operator consequence;
- Decodex implication;
- confidence and caveats;
- one recommended outcome: implement, test, document, publish, monitor, or no-op.

Do not create candidates, PRs, X posts, or task state from this skill. The owning
agent decides and uses the relevant hard boundary.
