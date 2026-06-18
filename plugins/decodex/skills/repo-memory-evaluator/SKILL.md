---
name: repo-memory-evaluator
description: Use when evaluating repo-memory OKF/LLM Wiki quality, route benchmarks, graph health, owner coverage, or curation impact.
---

# Repo Memory Evaluator

Evaluate whether a repo-memory OKF/LLM Wiki bundle is useful for real agent tasks.
The LLM designs benchmarks and judges owners; `decodex okf` only supplies evidence.

Use `repo-memory-writer` to create first-pass concepts, this skill to evaluate the
bundle, and `repo-memory-curator` to repair misses or graph decay.

Evaluate three layers:

- Static quality: check result, graph counts, broken links, orphans, duplicate owners.
- Retrieval benchmark: real task questions, expected owners, top-1/top-3 route hits.
- Usage outcome: fewer wrong reads, fewer duplicate claims, preserved evidence.

Workflow:

1. Resolve root/profile. Run `decodex okf check <root> --profile <profile>` and
   `decodex okf graph <root>`.
2. Build a benchmark from representative task questions, not keyword searches.
3. For each question, write the expected owner concept or acceptable owner set before
   looking at route output.
4. Run `decodex okf route <root> "<question>"` for every question. Use `--limit <n>`
   only when the benchmark intentionally changes the candidate count.
5. Score top-1/top-3. Classify misses as missing concept, weak metadata, noisy owner,
   missing link/index, duplicate owner, stale claim, or acceptable leaf.
6. Report `At a Glance`, `Why It Matters`, `Fix First`, benchmark table, graph health,
   and recommended next step.
7. If repairs are needed, switch to `repo-memory-curator` and rerun the same questions.

Benchmark table:

| Question | Expected owner | Top-1 | Top-3 hit | Classification | Fix first |
| --- | --- | --- | --- | --- | --- |

Quality bars:

- Minimum useful: checks pass, no broken links, common-task top-3 is reliable, and
  misses have clear fix categories.
- Good: common operational questions are top-1, specialized questions are top-3,
  orphans are intentional or queued, and metadata-only repairs improve route results
  without adding unrelated concepts.
- Not ready: generated summaries dominate, owners are unclear, route misses cannot be
  explained, graph links are decorative, or claims lack source/code/drift evidence.

Report remaining uncertainty honestly. Static checks prove shape, not usefulness.
