# Repository test-quality review

## Scope and completion rule

Review tracked test owners for source snapshots, literal-only assertions,
duplicate coverage, repeated fixtures, and timing-sensitive UI checks. Inspect
candidates against the production path and retained behavior checks. A search
hit is not a removal decision. A retained module is a valid review outcome.
This audit does not claim mutation coverage or a line-by-line security review.

| Batch | Owners | Status | Decision |
| --- | --- | --- | --- |
| Existing cleanup | GPUI, menu bar, static script gates, core/protocol | PR #1690 | Remove visual/source snapshots; preserve behavior; share socket setup |
| Tooling | CLI, scripts, automation | In review | Remove copied configuration values; retain actual process, file, and runtime reconciliation tests |
| Domain boundaries | Core, protocol, account login, FFI | Pending | Review duplicate contracts and fixtures |
| Execution | Codex adapter, runtime | Pending | Review native/fake-server distinctions and state-recovery coverage |
| Persistence | Database and transfer | Pending | Review migration fixtures and duplicate setup |
| Remaining desktop | GPUI and menu bar | Reviewed | Run shared application tests once; preserve preview-specific tests |
| Automation products | Radar and Publisher; site | Pending | Review parser, provenance, publication, and deployment protections |

## Tooling

- The portfolio test copied the current models, effort settings, IDs, and RRULE.
  Its DST check converted a hard-coded time and never used the rendered schedule.
  Check that rendering preserves manifest settings and per-entry overrides.
  Keep primary-worktree selection, manual pause, unexpected managed IDs, and
  runtime metadata/status checks.
- Remove the CLI installer test that only repeats `APP_HELPER`. Keep real symlink
  creation, idempotence, conflicting destination, and executable permission checks.
- Retain CLI process tests: help/version must exit without starting the service;
  build identity must decode; database commands must use the unified executable.
- Retain script tests for atomic writes, signing identity, redaction, source
  retention, quota conversion, diagnostic runner path resolution, and bundle identity.
- Retain the shell acceptance scripts: they launch actual processes or inspect
  staged bundles. They are not source snapshots.

## Desktop test ownership

- The visual-capture binary imports 569 application tests. The glass probe imports
  another 10. Their test names are all present in the main GPUI binary; neither
  tool defines its own test. The default nextest filter runs those tests in the
  main binary only. `--all-targets --all-features` still compiles the tools.
- `test = false` does not exclude these targets when `--all-targets` is used.
  Use the existing nextest configuration instead of a second test command.
- Keep the weather preview target: it has its own saved-payload parsing check.
- Remove the curated quote count assertion. Keep attribution, length, language,
  display, and non-repetition checks so the catalog can change without test edits.
- Retain menu bar interaction, account routing, reset-card lifecycle, and native
  credential/ownership boundary tests. Visual constants are not acceptance evidence.
