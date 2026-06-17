---
type: Evidence
title: Decodex Plugin Eval
description: Records plugin-eval results for the Decodex OKF docs and research skill migration.
status: active
authority: evidence
owner: docs
tags: [plugin-eval, skills, docs, research]
source_refs: []
code_refs: [plugins/decodex/.codex-plugin/plugin.json, plugins/decodex/skills/docs/SKILL.md, plugins/decodex/skills/docs-okf/SKILL.md, plugins/decodex/skills/docs-wiki/SKILL.md, plugins/decodex/skills/docs-drift/SKILL.md, plugins/decodex/skills/research/SKILL.md]
related: [../policy.md, ./docs-self-iteration.md]
last_verified: 2026-06-17
---

# Decodex Plugin Eval

Purpose: Preserve public-safe evidence that the Decodex plugin and the new docs skill
family passed local plugin evaluation after the OKF docs migration.

Read this when: You need proof that the docs skill split, research skill split, and
plugin invocation policy were evaluated before landing.

Not this document: A runtime benchmark, coverage report, or replacement for
`plugin-eval` output.

Covers: Static plugin-eval commands, score results, and the invocation-policy decision.

## Commands

```sh
node /Users/x/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/decodex --format json
node /Users/x/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/decodex/skills/docs --format json
node /Users/x/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/decodex/skills/docs-okf --format json
node /Users/x/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/decodex/skills/docs-wiki --format json
node /Users/x/.codex/plugins/cache/openai-curated-remote/plugin-eval/0.1.2/scripts/plugin-eval.js analyze plugins/decodex/skills/docs-drift --format json
```

## Results

| Target | Score | Grade | Risk | Fix First |
| --- | --- | --- | --- | --- |
| `plugins/decodex` | 100 | A | low | none |
| `plugins/decodex/skills/docs` | 100 | A | low | none |
| `plugins/decodex/skills/docs-okf` | 100 | A | low | none |
| `plugins/decodex/skills/docs-wiki` | 100 | A | low | none |
| `plugins/decodex/skills/docs-drift` | 100 | A | low | none |

## Invocation Policy

The plugin keeps the top-level router skills implicit and marks specialist skills as
explicit-only through local `agents/openai.yaml` files. This preserves direct
invocation while keeping plugin active-context cost bounded.

Implicit skills:

- `decodex`
- `docs`
- `planning`
- `research`

Explicit-only skills:

- `automation`
- `commit`
- `docs-drift`
- `docs-okf`
- `docs-wiki`
- `labels`
- `land`
- `manual-cli`
- `research-challenge`
- `research-decision`
- `research-evidence`
- `research-judgment`
- `research-options`
- `research-probe`
- `research-promote`

## Limits

The evaluation is static plugin analysis, not a measured real-usage benchmark. It is
sufficient for the skill/plugin change gate in this lane because plugin-eval reported
no warning or failing checks after the invocation-policy adjustment.
