---
name: repo-memory-evaluator
description: Use when evaluating repo-memory OKF/LLM Wiki quality, graph health, owner coverage, source evidence, or curation impact.
---

# Repo Memory Evaluator

Evaluate whether a repo-memory OKF/LLM Wiki bundle is useful for real agent tasks.
The LLM designs review questions and judges owners; `decodex okf` only supplies
shape, graph, and field-lookup evidence.

Use `repo-memory-writer` to create first-pass concepts, this skill to evaluate the
bundle, and `repo-memory-curator` to repair misses or graph decay.

Evaluate three layers without turning OKF into a retrieval system:

- Static quality: check result, graph counts, broken links, orphans, duplicate owners.
- Navigation review: real task questions, expected owners, index/link paths, and
  concrete field or text lookups that support discoverability.
- Usage outcome: fewer wrong reads, fewer duplicate claims, preserved evidence.

Workflow:

1. Resolve root/profile. Run `decodex okf check <root> --profile <profile>` and
   `decodex okf graph <root>`.
2. Build a review set from representative task questions, not keyword searches.
3. For each question, write the expected owner concept or acceptable owner set before
   inspecting the bundle path.
4. For each question, verify whether an agent can reach the owner through
   `index.md`, lane indexes, `related`, Markdown links, or precise
   `decodex okf find` filters.
5. Classify misses as missing concept, weak owner, missing link/index, duplicate
   owner, stale claim, weak evidence, or acceptable leaf.
6. Report `At a Glance`, `Why It Matters`, `Fix First`, review table, graph health,
   and recommended next step.
7. If repairs are needed, switch to `repo-memory-curator` and rerun the same review
   questions.

Review table:

| Question | Expected owner | Navigation path | Field/find evidence | Classification | Fix first |
| --- | --- | --- | --- | --- | --- |

Quality bars:

- Minimum useful: checks pass, no broken links, common-task owner concepts are
  reachable from indexes or links, and misses have clear fix categories.
- Good: common operational owners are obvious from the navigation graph, specialized
  owners are reachable through precise links or frontmatter fields, orphans are
  intentional or queued, and metadata-only repairs improve discoverability without
  adding unrelated concepts.
- Not ready: generated summaries dominate, owners are unclear, graph links are
  decorative, or claims lack source/code/drift evidence.

Report remaining uncertainty honestly. Static checks prove shape, not usefulness.
