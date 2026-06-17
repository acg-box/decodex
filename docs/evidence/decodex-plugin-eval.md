---
type: Evidence
title: Decodex Plugin Eval
description: Records plugin-eval results for the Decodex portable OKF, repo-memory, docs, and research skills.
status: active
authority: evidence
owner: docs
tags: [plugin-eval, skills, docs, research, okf, repo-memory]
source_refs: []
code_refs: [plugins/decodex/.codex-plugin/plugin.json, plugins/decodex/references/okf-layer.md, plugins/decodex/skills/okf/SKILL.md, plugins/decodex/skills/okf-query/SKILL.md, plugins/decodex/skills/okf-maintain/SKILL.md, plugins/decodex/skills/repo-memory-writer/SKILL.md, plugins/decodex/skills/docs/SKILL.md, plugins/decodex/skills/docs-okf/SKILL.md, plugins/decodex/skills/docs-wiki/SKILL.md, plugins/decodex/skills/docs-drift/SKILL.md, plugins/decodex/skills/research/SKILL.md]
related: [../policy.md, ./docs-self-iteration.md]
last_verified: 2026-06-17
---

# Decodex Plugin Eval

Purpose: Preserve public-safe evidence that the Decodex plugin, portable OKF init and
skill family, repo-memory writer skill, and docs skill family passed local plugin
evaluation.

Read this when: You need proof that the OKF init/split, repo-memory writer, docs skill
split, research skill split, and plugin invocation policy were evaluated before
landing.

Not this document: A runtime benchmark, coverage report, or replacement for
`plugin-eval` output.

Covers: Static plugin-eval commands, score results, and the invocation-policy decision.

## Commands

```sh
node /Users/x/.codex/plugins/cache/openai-curated/plugin-eval/43313cc9/scripts/plugin-eval.js analyze plugins/decodex --format json
node /Users/x/.codex/plugins/cache/openai-curated/plugin-eval/43313cc9/scripts/plugin-eval.js analyze plugins/decodex/skills/okf --format json
node /Users/x/.codex/plugins/cache/openai-curated/plugin-eval/43313cc9/scripts/plugin-eval.js analyze plugins/decodex/skills/okf-query --format json
node /Users/x/.codex/plugins/cache/openai-curated/plugin-eval/43313cc9/scripts/plugin-eval.js analyze plugins/decodex/skills/okf-maintain --format json
node /Users/x/.codex/plugins/cache/openai-curated/plugin-eval/43313cc9/scripts/plugin-eval.js analyze plugins/decodex/skills/repo-memory-writer --format markdown
node /Users/x/.codex/plugins/cache/openai-curated/plugin-eval/43313cc9/scripts/plugin-eval.js analyze plugins/decodex/skills/docs --format json
node /Users/x/.codex/plugins/cache/openai-curated/plugin-eval/43313cc9/scripts/plugin-eval.js analyze plugins/decodex/skills/docs-okf --format json
node /Users/x/.codex/plugins/cache/openai-curated/plugin-eval/43313cc9/scripts/plugin-eval.js analyze plugins/decodex/skills/docs-wiki --format json
node /Users/x/.codex/plugins/cache/openai-curated/plugin-eval/43313cc9/scripts/plugin-eval.js analyze plugins/decodex/skills/docs-drift --format json
```

## Results

| Target | Score | Grade | Risk | Fix First |
| --- | --- | --- | --- | --- |
| `plugins/decodex` | 100 | A | low | none |
| `plugins/decodex/skills/okf` | 100 | A | low | none |
| `plugins/decodex/skills/okf-query` | 100 | A | low | none |
| `plugins/decodex/skills/okf-maintain` | 100 | A | low | none |
| `plugins/decodex/skills/repo-memory-writer` | 100 | A | low | none |
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
- `okf`
- `okf-maintain`
- `okf-query`
- `repo-memory-writer`
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
no warning or failing checks after the OKF split, repo-memory writer addition, and
invocation-policy adjustment.
