Keep Decodex up to date with official openai/codex.

Start each run from freshly fetched Decodex main. Check merged changes, open PRs and known related tasks before identifying gaps. Reuse existing implementations; record separately what is implemented, what needs behavioral validation, and what needs adaptation. Avoid duplicating active work; continue independent upstream review while overlapping changes are pending.

Establish the review baseline from current Decodex source, merged changes and upstream evidence. Treat old cursors as unverified until checked. Read upstream commits and diffs in consecutive batches; trace relevant implementation, protocol contracts and tests into Decodex's actual consumers.

Cover behavior and useful capabilities, including async input and replies, files and thread attachments, usage and compaction, model discovery and execution settings, plugins/MCP, authentication and native agents. Distinguish released behavior from main-only APIs. A schema check is not proof of working behavior. Record concrete gaps and applicability decisions; do not call the integration caught up while relevant work is unread or unfinished.

Implement the needed adaptations, test their actual behavior, review the patch, split independent capabilities into focused commits and PRs, and merge each batch when its repository requirements pass. Do not accumulate completed independent work behind an unfinished catch-up batch. Use signed commits and a normal merge with a clear imperative commit message. Verify that remote main contains the delivered change. Use current repository commands and architecture; do not revive retired code or obsolete PR prerequisites.

Keep durable review records in this automation's directory. memory.md is the short resume index: upstream range, Decodex revision, last fully reviewed commit, exact next commit, capability gaps, pending work and PR/merge status. Save progress after each batch. Track unread commits separately from delivery blockers so neither is lost.

Report useful merged changes or actionable blockers concisely; stay quiet when nothing changes. Do not edit OpenWiki. The website is retired and will be redesigned separately; website dependencies and site checks are outside this task.

Scope policy: Necessary core app-server and protocol adaptations must be tied to an existing Decodex consumer and concrete compatibility or correctness impact. Native-owned behavior remains in Codex. Optional new product features must be reported to the user with their benefit, implementation scope and maintenance cost; do not implement them until the user selects them. Scanning an upstream commit does not mean adopting it. Maintain a capability adoption inventory with core versus optional classification, source owners, PRs and merge/validation status.

This automation is paused at the user's request. Completion of the current manual fixed-cutoff update does not authorize enabling it. Wait for explicit later user instruction before resumption.
