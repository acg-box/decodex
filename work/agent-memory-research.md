# Agent memory research for Decodex

Research date: 2026-09-16. This is a source review and a proposed evaluation plan,
not an integration, deployment, or measured product result. Existing memory
settings remain unchanged. Casual user suggestions are candidates, not approval
to enable memory or permanent preferences.

## Findings

There is no established universal winner in the sources reviewed. Coding agents,
personal assistants, and temporal business knowledge have different requirements.
Vendor results use different models, tasks, context sizes, and retrieval budgets.

| Approach | Evidence | Implication for Decodex (assessment) |
| --- | --- | --- |
| Claude Code | Explicit instruction files plus per-repository automatic notes; bounded initial loading. [Docs](https://code.claude.com/docs/en/memory) | Small visible context plus on-demand evidence retrieval is a credible baseline. |
| Letta | Persistent editable memory blocks can be shared among agents. [SDK docs](https://docs.letta.com/api/typescript) | Useful shared-context design; adopting its runtime would add a second agent-management owner. |
| Mem0 | Extracts, consolidates, and retrieves conversational information; reports LoCoMo results. Explicit update operations exist. [Paper](https://arxiv.org/abs/2504.19413), [update API](https://docs.mem0.ai/core-concepts/memory-operations/update) | Candidate for learned user facts; conversational recall scores do not establish coding-project correctness. |
| Graphiti / Zep | Temporal facts, relationships, invalidation, and hybrid retrieval. [Overview](https://help.getzep.com/graphiti/getting-started/overview) | Most relevant when changing entity relationships become a demonstrated requirement. The documented Graphiti MCP implementation is experimental. [MCP docs](https://help.getzep.com/graphiti/getting-started/mcp-server) |
| Hindsight | Distinguishes evidence and synthesized beliefs; retain, recall, and reflect operations. [Paper](https://arxiv.org/abs/2512.12818) | First external candidate to evaluate for traceable project context, not a declared winner. |
| LangGraph | Thread state, namespaced long-term storage, and synchronous/background writes. [Docs](https://docs.langchain.com/oss/python/concepts/memory) | Scope and freshness should be explicit even without adopting this framework. |

Codex Memory v2 remains a separate candidate based on the source review recorded
in this task. Public main, installed binary, enabled features, and backend access
must be verified separately. Do not enable the user's disabled memory implicitly.

## Evaluation evidence

LongMemEval tests extraction, cross-session and temporal reasoning, updates, and
abstention. [Paper](https://arxiv.org/abs/2410.10813)

LongMemEval-V2 evaluates experience with web environments, including changing state,
workflows, failure modes, and false premises. Its authors report 72.5% average
accuracy for their file-backed AgentRunbook-C, versus 48.5% for their strongest RAG
baseline and 69.3% for the off-the-shelf coding-agent baseline, with high latency.
This work-in-progress result is scoped to that benchmark and is not a general
ranking of memory products. [Paper](https://arxiv.org/abs/2605.12493)

## Recommended product boundary

Decodex owns source conversations, work state, review evidence, confirmed decisions,
and their project scope. Memory retrieval returns supporting context; it cannot
approve work, change permissions, or silently turn a suggestion into a decision.
A short project context view is a projection, not a second independent truth store.
Current task state comes from its owner rather than an inferred memory fact.

Start with a small relevant context and retrieve additional evidence when needed.
This follows the documented just-in-time context approach. [Anthropic engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)

## Candidate trial

Compare the existing-record retrieval baseline against one isolated Hindsight
integration. Consider Codex v2 separately after explicit enablement and runtime
support checks. Do not run several automatic memory writers against the same
project during the first trial.

Hindsight has native MCP and project-bank routing. Bank selection is a retrieval
boundary, not a replacement for authorization. Use the minimum tool surface and
verify server-enforced access; do not expose administrative operations by default.
[Server docs](https://hindsight.vectorize.io/developer/mcp-server)

MCP does not establish free inference or Codex subscription coverage. Evaluate
extraction, embedding, reflection, storage, latency, and operating costs separately.
The cloud documentation describes token-metered memory operations.
[Cloud docs](https://docs.hindsight.vectorize.io/mcp/)

Proposed trial: 30-50 controlled cases with known answers, followed by real task
validation. Include temporary versus durable requests, corrected decisions, project
isolation, proposal versus implementation, version-bound review findings, missing
evidence, explicit forgetting, context compaction, worker changes, and restarts.
Measure correctness, supported citations, false claims, wrong-project retrieval,
latency, token cost, and user corrections. Compare at matched model and retrieval
budgets; do not infer superiority from incomparable published scores.

Promote a candidate only if it improves real task continuity and reduces user
corrections at acceptable cost, without regressing scope or evidence handling.
No provider, MCP server, account, model, or memory setting was installed or changed
as part of this research.

## Native-first decision and runtime check (2026-09-17)

The user enabled Codex Memory. Do not add an external memory service in this version.
Track comparative evaluation in https://github.com/acg-box/decodex/issues/1336.

A live Decodex-owned Codex connection returned `memories.enabled = true` through
`experimentalFeature/list`. The same connection returned five visible models through
`model/list`. This verifies the effective feature flag in the runtime, not v2 backend
selection, recall quality, or cross-project memory isolation. No user setting was changed.
The earlier candidate comparison remains research, not a selected integration.
