# Codex upstream synchronization

One daily task, `codex-upstream-maintainer`, owns source review, adaptation,
validation, PR merge and the resume record. There is no separate scheduled
reviewer or health role. The task uses the current maintainer model and effort
from `automations/portfolio.toml` and runs in a Codex-managed project worktree.

The schedule is UTC 20:05, which is Beijing 04:05 the next day throughout the
year. The explicit UTC start prevents daylight-saving changes. The maintainer
remains paused during the manual catch-up. After all required changes are
verified and merged, update its manifest status and native definition to ACTIVE.
Do not activate it merely because a source-review batch or schema check passes.

The maintainer reads consecutive official Codex commits and traces relevant
behavior into current Decodex consumers. It checks merged and concurrent work
before adding an implementation. Native Codex owns supported execution behavior;
Decodex owns its presentation and coordination. The installed binary, upstream
main, local tests and delivered behavior are separate evidence scopes.

Keep `memory.md` in the native automation directory as the short resume index.
Record the reviewed range, exact next commit, implementation and validation gaps,
Decodex revision and PR/merge state. Store longer batch evidence separately.
A source-review cursor must not imply that all adaptation or acceptance is done.
No-op runs stay quiet; report useful merged changes or actionable blockers.

The portfolio also retains the independent content manager and publisher
configuration. This upstream task does not manage those roles, OpenWiki, or the
retired website. Rendering the portfolio is read-only; it does not register or
activate tasks. Use native automation tools for an explicitly selected definition
and preserve the user's current pause and notification settings.

Validation commands:

```sh
python3 automations/decodex/scripts/config/render_automation_plan.py --json
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo make test-automations
```

The runtime evaluator reports drift; it does not repair, delete, activate, or
create native definitions. Missing unrelated content tasks are not authority to
register them as part of upstream synchronization.
