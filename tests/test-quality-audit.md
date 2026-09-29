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
| Domain boundaries | Core, protocol, account login, FFI | Reviewed | Retain wire, digest, ABI, callback, and file-integrity contracts |
| Execution | Codex adapter, runtime | Reviewed | Share identical native attestation setup; preserve initialization order and native/fake-server boundaries |
| Persistence | Database and transfer | Reviewed | Share repeated bound-work and process-death setup; keep migration and recovery assertions |
| Remaining desktop | GPUI and menu bar | Reviewed | Run shared application tests once; preserve preview-specific tests |
| Automation products | Radar and Publisher; site | Reviewed | Remove obsolete wording scans; preserve parser, provenance, and publication checks |

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

## Domain, execution, and persistence

- Consolidate the identical process identity/death fixture used by model, plugin,
  permission, and prompt-edit recovery tests. Each scenario keeps its own native
  observations, restart, stale-owner, and lost-reply assertions.
- Share the bound-work store fixture used by model, plugin, and permission tests.
  Keep each feature's publication and receipt assertions local.
- Reuse native executable attestation and synthetic credentials. Keep the
  activation-policy test's separate launch sequence: it applies policy before
  initialization, unlike the ordinary control fixture.
- Keep historical migration SQL fixtures. Creating a current schema instead would
  remove evidence that persisted older data survives migration.
- Keep canonical digest, public ABI, bounded decoding, OAuth callback, credential
  redaction, filesystem identity, and transfer tests. Exact values are contracts
  in these cases, not incidental implementation snapshots.
- CLI and FFI fast-mode tests have similar bodies but call separate implementations
  with different file access paths. Neither test suite replaces the other.
- Native adapter tests and runtime tests also cross different authority boundaries.
  Their approval and completion helpers stay local; no shared cross-crate test
  framework is introduced for two small helpers.
- Retain the local-transport fixture in both protocol test modules. Consolidating
  two small setup functions would introduce a module dependency without removing
  duplicate behavior checks.
- Real native tests remain opt-in where they require an installed binary, signing,
  or external service. Compilation does not claim live acceptance of those tests.

## Automation products

- Remove two Radar tests that scanned application source, generated content, and
  OpenWiki pages for retired words. These reject valid historical explanations
  and do not prove runtime behavior. Remove their now-unused traversal helpers.
- Keep actual artifact validation for legacy tool references, obsolete schema,
  duplicate slugs, material refresh comparison, provenance, and review states.
- Keep Publisher dispatch idempotence, receipts, file identity, authority,
  payload bounds, and paid-publication checks. The static raw-client boundary
  check remains a narrow anti-bypass check, not proof of publication behavior.
- Site has no separate tracked unit-test suite. No site build or deployment gate
  is removed by this cleanup.
