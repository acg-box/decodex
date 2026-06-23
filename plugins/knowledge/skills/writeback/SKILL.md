---
name: writeback
description: Use when stable implementation, debugging, research, or drift findings should update docs, OKF/LLM Wiki, repo-memory, logs, or a knowledge candidate queue.
---

# Knowledge Writeback

Close the loop between implementation and durable knowledge. Write back stable,
source-backed facts; keep uncertain or local-only observations as candidates or
explicit gaps.

## Rules

- Use `$knowledge:docs` for checked-in repository docs owners.
- Use `$knowledge:okf` for portable OKF/LLM Wiki bundles.
- Use `$knowledge:repo-memory` for source-backed repository memory.
- Use `$knowledge:docs-drift` when a changed claim may diverge from code, config,
  commands, generated artifacts, status text, or runtime behavior.
- Do not write speculative lessons as authoritative docs. Record unresolved material
  gaps as candidates, drift blockers, or follow-up evidence needs.
- Prefer automatic writeback when evidence is strong and the target owner is clear.
  Ask the user only for human-only product or authority choices.

## Output

Report owner target, source evidence, writeback performed or candidate recorded,
validation command, and remaining drift or authority gaps.
