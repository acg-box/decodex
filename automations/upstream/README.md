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

Development can use official Codex main at a fixed reviewed commit before a
Release exists. The actual artifact and its helpers must be qualified, preserve
the fixes already merged into Decodex, and pass the applicable native regressions.
Formal Decodex releases use official Codex Releases and must preserve the current
merged baseline. An older Release is not an automatic fallback. Development
artifacts are prepared explicitly; app packaging and startup do not compile Codex.
Both use the same component verification and cache repair. The
[maintainer prompt](prompts/maintainer.md) owns the qualification and merge
requirements. Tag creation, installation and publication remain outside this task.

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

Keep the portable retention index in [retained-evidence.json](retained-evidence.json).
It records retained behavior requirements, exact validation entrypoints, artifact
identities and qualification limits. It is not the live cursor or a task journal.
Update current entries and let Git retain older versions. Keep raw logs, build
products, personal paths, real account data and native scheduler state outside the
repository. Preserve reusable custom regression fixtures with their test owner.

Build directories and old local receipts can be removed. If they are absent,
continue independent source review and reconstruct only the affected proof from
the index, current source, tests and public delivery records. Prepare the exact
artifact again when required. A rebuilt artifact needs its own qualification;
do not treat it as the deleted original or require the user to restore old builds.
Scope an evidence gap to the action that needs it. Never infer a new source-review
cursor or a passed test from missing logs.

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
