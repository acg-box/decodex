> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Retire the superseded native timeline PR

Original PR: https://github.com/acg-box/decodex/pull/1378
Original head: `4e370c07464ea3528ed1334fd6ce75fcc5ca595a`.
Comparison main: `3dac76a7d39957bf809e9925934969b6e3b1f63e`.

## Disposition

The old PR must not be merged wholesale over the current owners. Its 73 commits
have 12 patch-equivalent matches on main and 61 non-equivalent patches. Patch
inequality is not a missing-feature count. The
[commit register](pr1378-commit-disposition.tsv) records every commit and its
current owners. The [path register](pr1378-path-disposition.tsv) covers all 171
changed paths, including 65 paths outside the earlier 360-path inherited register.
All referenced current owners exist.

The review found one useful omitted regression: package-style MCP OAuth names.
Restore the original test from `852b55eeb`. It checks the exact native login name,
rejects a normalized alias as completion authority, accepts the original name,
and rejects duplicate native requests. The production owner already supports the
behavior. No other production capability needs to be copied back from this PR.

## Material replacements

| Original behavior | Current disposition |
| --- | --- |
| Native timeline, usage, media and scroll anchors | Retained in Agent timeline, transcript and media owners. Relative media uses the admitted process directory; scrolling cancels old interpolation after a prepend. Existing inherited-file dispositions cover shared files. |
| Native goals and budget counters | `agent_native_goal` protocol, runtime and desktop owners replace `chief_goal`. Exact source/thread checks, child ancestry, unavailable versus empty results and explicit refresh remain. |
| Native forms and request liveness | Form negotiation is restricted to the retained Agent connection. Ordinary readers do not advertise an unsupported form consumer. Host request guards and reused-RPC tests replace coordinator-local liveness assumptions. |
| App approval settings and live reviewer edits | Shared config journals, immutable outcomes and source-bound native review replace the old task-local account journal. Saved edits do not answer approval requests. Current and legacy receipts remain readable. |
| Model recovery and task settings | One current model owner replaces the old writable recovery journal. The legacy reader retains unresolved requests, account provenance and no-replay guards. See the existing model-owner and legacy-journal reconciliation records. |
| Persistent reasoning effort | The extensible native effort owner replaces a fixed allowlist. The old test's rejection of every unlisted effort is obsolete. |
| Live plans and compaction | Current transcript/output owners retain unfinished plans and authoritative completion; selected-turn compaction stays visible until its matching completion. Current tests cover later rendering and storage changes. |
| Closing-thread recovery | Current exact-thread/native-error retry, owned-source checks and deferred recovery extend the old behavior without input replay. |
| Database migrations 31–33 | Schema 48 baseline includes live output kinds and independent account usage facts. Current schema 49 and existing source-upgrade tests own subsequent migration behavior; do not reintroduce obsolete numbered migrations. |
| Account routing | Existing independent usage permission, credits, spending and ownership rules remain. The final old routing patch is patch-equivalent on main. |

## Stashes and source snapshot

The two authorized retirement candidates are:

- `9187391b7ffc569f8d304ae3bfdfea5c32d566cc`: 76 tracked and 48 untracked paths.
  Every path is in the completed inherited-file register. Older model-recovery
  writer and test names map to the current model owner and legacy receipt reader.
- `3a9454d5457882473cb49697872ea6403ecf4b29`: 26 tracked and 15 untracked paths.
  The 12 paths outside that register are included in this PR's path comparison.
  Its added function names remain in current source after the Agent rename.

Before deleting the old branch or stash references, retain their exact Git objects
in one verified recovery bundle and retain the 357-file source snapshot in a
compressed archive. This permits removal of active staging references without
losing original bytes. Existing audit documents use historical names and hashes;
these records must not be rewritten as if the old branch was merged directly.

## Validation and cleanup boundary

The three focused MCP login tests pass, including the restored regression.
Runtime library/test Clippy passes with warnings denied. No broad desktop rerun is
needed for this test-only recovery. Check all register owners and relative links,
then use the normal signed-commit and PR checks before closing the old PR.

After merge, close PR1378 as superseded and link this record. Remove only reviewed,
merged branches from this manual catch-up and the exact archived original branch
and stashes. Preserve unrelated checkouts, branches, installed application data and
the paused maintenance configuration. Verify closure, branch/stash removal and
worktree cleanup separately. Optional feature removal remains a separate product
decision; this retirement does not remove adopted features.
