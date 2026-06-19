---
type: Evidence
title: Decodex Plugin Eval
description: Records plugin-eval results for the Decodex plugin, routing reference, repo-work, portable OKF, repo-memory, docs drift, debugging, and research skills.
status: active
authority: evidence
owner: docs
tags: [plugin-eval, skills, repo-work, docs, research, okf, repo-memory, semantic-drift, debugging]
source_refs: []
code_refs: [plugins/decodex/.codex-plugin/plugin.json, plugins/decodex/references/routing.md, plugins/decodex/references/repo-workflow.md, plugins/decodex/references/dep-roll-policy.md, plugins/decodex/references/okf-layer.md, plugins/decodex/references/docs-drift.md, plugins/decodex/skills/decodex/SKILL.md, plugins/decodex/skills/repo-work/SKILL.md, plugins/decodex/skills/decodex-ops/SKILL.md, plugins/decodex/skills/dep-roll/SKILL.md, plugins/decodex/skills/dep-style/SKILL.md, plugins/decodex/skills/review-feedback/SKILL.md, plugins/decodex/skills/verification/SKILL.md, plugins/decodex/skills/okf/SKILL.md, plugins/decodex/skills/okf-query/SKILL.md, plugins/decodex/skills/okf-maintain/SKILL.md, plugins/decodex/skills/repo-memory-writer/SKILL.md, plugins/decodex/skills/repo-memory-evaluator/SKILL.md, plugins/decodex/skills/repo-memory-curator/SKILL.md, plugins/decodex/skills/docs/SKILL.md, plugins/decodex/skills/docs-okf/SKILL.md, plugins/decodex/skills/docs-wiki/SKILL.md, plugins/decodex/skills/docs-drift/SKILL.md, plugins/decodex/skills/debugging/SKILL.md, plugins/decodex/skills/research/SKILL.md, plugins/decodex/skills/research-challenge/SKILL.md, plugins/decodex/scripts/semantic_drift_audit.py]
related: [../policy.md, ./docs-self-iteration.md]
last_verified: 2026-06-19
---

# Decodex Plugin Eval

Purpose: Preserve public-safe evidence that the Decodex plugin, routing reference,
portable OKF init and skill family, repo-work skill family, repo-memory skill family,
docs drift skill, debugging skill, and research skill family passed local plugin
evaluation without failures.

Read this when: You need proof that the OKF init/split, repo-memory skills, docs
drift, debugging, repo-work migration, research skill split, and plugin invocation
policy were evaluated before landing.

Not this document: A runtime benchmark, coverage report, or replacement for
`plugin-eval` output.

Covers: Static plugin-eval commands, score results, and the invocation-policy decision.

## Commands

Current full-plugin gate:

```sh
node ~/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/decodex --format markdown
```

## Results

| Target | Score | Grade | Risk | Checks | Fix First |
| --- | ---: | --- | --- | --- | --- |
| `plugins/decodex` | 95 | A | medium | 0 fail, 1 warn, 2 info | Reduce deferred token budget in a future slimming pass. |

Static budget snapshot:

- Active invocation budget: 1035 tokens.
- Deferred skill budget: 10821 tokens.
- Explicit-only invocation budget: 10280 tokens.
- Plugin skill count: 29 skills, with one implicit router and 28 explicit-only skills.

## Invocation Policy

The plugin keeps the top-level router skills implicit and marks specialist skills as
explicit-only through local `agents/openai.yaml` files. This preserves direct
invocation while keeping plugin active-context cost bounded.

Implicit skills:

- `decodex`

Explicit-only skills:

- `commit`
- `debugging`
- `decodex-ops`
- `dep-roll`
- `dep-style`
- `docs`
- `docs-drift`
- `docs-okf`
- `docs-wiki`
- `land`
- `okf`
- `okf-maintain`
- `okf-query`
- `planning`
- `repo-memory-curator`
- `repo-memory-evaluator`
- `repo-memory-writer`
- `repo-work`
- `review-feedback`
- `research`
- `research-challenge`
- `research-decision`
- `research-evidence`
- `research-judgment`
- `research-options`
- `research-probe`
- `research-promote`
- `verification`

## Limits

The evaluation is static plugin analysis, not a measured real-usage benchmark. The
2026-06-19 full-plugin rerun after the Decodex ops consolidation reported score
95/100, grade A, medium risk, zero failing checks, one warning, and two informational
notes. Active invocation cost was 1035 tokens; deferred skill cost was 10821 tokens;
explicit-only invocation cost was 10280 tokens. The remaining warning is
`deferred_cost_tokens-budget-high`, a known static token-budget cleanup item after
the repo-work migration, not a routing, safety, or progressive-disclosure failure.
The manifest default prompt count is 3, which keeps repo-work, docs/OKF, research,
and debugging starters inside the first-three prompt surface used by Codex. Only the
top-level `decodex` router remains implicit; repo-work, docs, planning, Decodex ops,
repo-memory, research, and specialist skills are explicit-only because the top-level
router and host `AGENTS.md` name the narrower owner skills directly.

Directly evaluating the installed cache path
`~/.codex/plugins/cache/hack-ink/decodex/0.2.0` reports an additional
`manifest-name-directory-mismatch` warning because the plugin manager stores the
plugin under a version directory. Use the source root `plugins/decodex` for the
canonical plugin-eval score.
