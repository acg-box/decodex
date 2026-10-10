# Codex upstream synchronization

One daily task, `codex-upstream-maintainer`, owns source review, adaptation,
validation, PR merge and the resume record. There is no separate scheduled
reviewer or health role. The task uses the current maintainer model and effort
from `automations/portfolio.toml` and runs in a Codex-managed project worktree.

The schedule is UTC 20:05, which is Beijing 04:05 the next day throughout the
year. The explicit UTC start prevents daylight-saving changes. The portfolio
declares the desired configuration; inspect the native definition for its current
status. Preserve the user's pause decision. A source-review batch, schema check
or manifest status does not authorize activation.

Development follows official Codex main at an exact reviewed commit. Necessary
runtime and compatibility updates can merge after qualification without waiting
for a stable tag. The actual runtime artifact must match that commit. Tag creation,
release cadence, installation and publication are outside this task.

The maintainer reads consecutive official Codex commits and traces relevant
behavior into current Decodex consumers. It checks merged and concurrent work
before adding an implementation. Native Codex owns supported execution behavior;
Decodex owns its presentation and coordination. The installed binary, upstream
main, local tests and delivered behavior are separate evidence scopes.

Use `state.json` in the native automation directory as the current cursor and
pending-work index, as specified by the [maintainer prompt](prompts/maintainer.md).
Treat older `memory.md` and dated records as historical evidence. Store longer
batch evidence separately.
A source-review cursor must not imply that all adaptation or acceptance is done.
No-op runs stay quiet; report useful merged changes or actionable blockers.

The portfolio also retains the independent content manager and publisher
configuration. This upstream task does not manage those roles, OpenWiki, or the
website. Rendering the portfolio is read-only; it does not register or
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
