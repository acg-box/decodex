# Codex Upstream Automations

This directory is the canonical source for the standalone Codex upstream adaptation
loop. The loop does not use Decodex server, runtime intake, Linear, or tracker state.
It uses the installed `decodex` CLI only for signed commits and reviewed landing.

The active roles are:

- `codex-upstream-maintainer`: observes upstream and the installed Codex schema,
  implements and stages one claimed compatibility change, and asks the state wrapper
  to validate it, commit it with `decodex commit`, and open a pull request.
- `codex-upstream-reviewer`: independently reviews the exact pull-request head,
  reproduces no-change decisions, requests bounded repairs, or lands it with
  `decodex land`.
- `codex-upstream-health`: checks cursor continuity, stale work, leases, and current
  Codex build evidence. It also reconciles the five exact live task definitions in
  the upstream and content manifests through the native Codex App lifecycle tool and
  verifies each readback.

All scheduled tasks run from the primary clean `main` checkout with local execution
and high reasoning. They are never configured with a worktree cwd. Maintainer and
Reviewer runs can create temporary task or review worktrees below `.worktrees`.

Generated state belongs under `.agent/automations/upstream/cache`. It is bounded and
stores SHA values, versions, schema fingerprints, pull-request URLs, trusted affected
path-category prefixes, exact HEAD/tree validation receipts, hashed leases,
generation-bound external-effect intents, and result codes. It does not store
individual upstream path names or prose, free-form commands, prompts, logs, account
identifiers, credentials, or personal data. It is not uploaded to GitHub.
Content-addressed installed-schema evidence is also local-only. It binds the resolved
Codex executable digest, normalized schema data, and complete file-digest manifest.
The evidence store is locked and limited to 512 files and 512 MiB. Deterministic
pruning preserves local-build and nonterminal candidate evidence and reserves two
maximum-size objects, by both file count and bytes, for the next stable and
experimental observation. Old terminal candidates
retain fingerprints but can release their evidence-file references.
Every save fsyncs a recovery slot and then the primary slot. A monotonic persistence
generation recovers the newest valid state after a process or power failure.

The source state separates the latest observed upstream head, the latest head covered
by queued contiguous ranges, and the latest terminal cursor. At most 128 source ranges
can be active at once. Later observations continue queueing from the covered head, so
a long downtime cannot be truncated or overflow the state.
Each batch records schema facts from its own terminal SHA. Release candidates bind
the tag name and resolved tag commit. A monotonic discovery sequence prevents a
schema A-to-B-to-A transition or a tag retarget from reusing an old terminal result.
The first observation queues independent main/bootstrap, stable-release, and
prerelease-release candidates, so a new installation does not wait for the next tag.

Health does not stop at reporting. Deterministic seven-day thresholds can queue one
deduplicated improvement candidate for repeated blocked attempts, repeated Reviewer
repairs, or sustained lead-time failure with enough samples. A remaining live
configuration mismatch queues the same bounded workflow. Maintainer must reproduce
the evidence and add a test; Reviewer must approve the result.

Commit, publish, pull-request retirement, and landing are transactional state-tool
commands. Each command persists an intent before its external effect, binds it to the
current lease generation, and requires exact readback. A retry adopts only the same
intent. An initial publish accepts only an absent remote branch or the exact candidate
base. A repair push binds the recorded prior remote head with force-with-lease.
Commit and landing bind the installed Decodex version and executable digest to their
intents. Commit uses the resolved absolute executable and requires a completed
execution receipt before it accepts the signed commit. Activation also requires the
pinned executable's `commit --help` and `land --help` surfaces to prove local,
server-independent manual authority and exact base/head landing arguments. Landing
uses a fresh 21,000-second lease budget. The wrapper first persists the exact land
intent. It then invokes the policy-pinned local `decodex land` command with the
reviewed base and head object IDs. The Decodex command, and no wrapper code, creates
the signed merge commit, pushes it, verifies it, synchronizes primary `main`, and
cleans the exact lane. The merge tree is the reviewed tree. Its parents are exactly
the validated base and reviewed head. A push with an exact
`--force-with-lease` expected old object ID is an atomic base compare-and-swap. A
concurrent `main` advance rejects the push before a merge occurs.

Each claim also creates a separate one-time subagent handoff challenge. The lease
token stays with the parent automation. A worker receipt binds the challenge to the
candidate, claim generation, original base, staged tree, and staged-path digest. An
independent Reviewer receipt binds it to the candidate, exact base/head/tree,
disposition, and bounded finding codes. Runtime state stores only the challenge
digest and sanitized receipt provenance. These receipts are non-replayable,
state-bound handoffs. They are not cryptographic identity signatures. A prepared
commit or started land effect keeps its original handoff receipt and intent
generation across lease recovery; a new owner generation can resume only that exact
intent and cannot replace its receipt.

The wrapper requires exact Decodex command output, the merged pull-request head and
merge SHA, remote-main containment, and an exact JSON landed-change record that
includes the unique land intent digest. A pull request that is already merged before
a fresh land intent is rejected. An open PR cannot reuse a Reviewer receipt after
`main` or the validation authority changes. After a `land_started` crash, the same
Decodex command can resume from the exact task worktree. If Decodex already removed
that worktree, it can complete readback and cleanup from primary `main`. The wrapper
can recognize only the exact intent-bound signed merge; it never creates a merge or
deletes a lane. If another authorized change advances `main` after that merge,
recovery requires the exact merge to remain an ancestor of the current remote tip and
fast-forwards primary to that tip. A rewritten or unrelated lineage fails closed.
Dirty, ambiguous, unowned, or out-of-root lanes are preserved and fail closed. The
wrapper records the command receipt before it resolves the candidate. Maintainer and
Reviewer
receipts bind the base, changed-path classification, current primary validation
authority, exact HEAD/tree, and all required profiles. The authority runs the base
profiles through its own `Makefile.toml`, so a candidate cannot weaken its gates.
GPUI, dependency, Apple build, and validation-authority changes add full
`cargo make check`. Before npm installation, the primary trusted audit validates the
fixed root package shape, scripts, dependency map, registries, integrity, and complete
lock graph. Install scripts are disabled. Maintainer and Reviewer never execute
candidate code directly. Validation uses a credential-scrubbed environment, a fixed
trusted tool discovery path, and a deny-default macOS sandbox. The sandbox denies
external network and personal-root reads. It allows only the exact candidate,
trusted Git data, toolchains, system runtime files, and private temporary build
outputs. Cargo source caches are read-only during candidate execution. Each profile
records and
rechecks the exact tool binaries, environment digest, fixed command digest, explicit
zero exit code, and bounded output digest. A terminal landed result retains the complete
bounded execution receipt and its digest.

## Validation

Validate source:

```sh
cargo make check-upstream-automation
python3 automations/decodex/scripts/config/evaluate_automations.py \
  --manifest automations/upstream/automations.toml --repo-only
```

The headless gate excludes `decodex-gpui` because an Apple GPUI build requires full
Xcode and Metal tools. Any change to GPUI, its dependencies, or Apple GPU/build
integration must use the full `cargo make check` gate on a host with those tools.
The trusted validator discovers a configured `DEVELOPER_DIR`, selected Xcode, or at
most 16 `Xcode*.app` installations. It runs the full gate with the first installation
that exposes Metal and binds absolute `xcode-select`, `xcrun`, `xcodebuild`, Xcode
version, and Metal evidence to the receipt.
Both gates validate the exact Node and npm runtime before any npm command. They also
include npm advisory, lock provenance, lifecycle-script metadata, and
registry-signature checks. The provenance gate verifies installed package identity
against the lock, rejects package-path symlinks, and pins the reviewed native and
platform package name, registry URL, integrity, OS, and CPU set. The site pins npm
11.17.0 and requires Node 22.12.0 or newer.
Run either aggregate on a clean committed tree because repository authority tests
bind their evidence to the exact commit and tree.

Validate live configuration:

```sh
python3 automations/decodex/scripts/config/evaluate_automations.py \
  --manifest automations/upstream/automations.toml
python3 automations/decodex/scripts/config/evaluate_automations.py \
  --manifest automations/decodex/automations.toml
```

The default sync command renders this manifest and the current two-task content
manifest. Live task creation and changes in Codex Desktop use the native automation
lifecycle tool. Health can create or repair only the five fixed IDs in those
manifests. It must read back each mutation. It never lists, edits, or deletes unrelated
tasks and never writes scheduler files directly. The renderer remains a portable
recovery and audit path and preserves `created_at`
metadata. The live evaluator rejects missing or invalid Codex App list timestamps.
Health also queues a bounded `content_loop_degraded` repair when validated content
evidence misses its freshness, publication, outcome, or account-restoration service
level. The candidate stores only bounded degradation codes, so Maintainer can
reproduce and repair the fault without reading social content.

## Scheduled Run Threads

Maintainer, Reviewer, Health, Content Manager, and Publisher apply the shared
`scheduled-run-thread-retention.md` policy. A complete terminal run calls native
`set_thread_archived` for its current thread after all durable and external-effect
readbacks. A run stays visible when it needs human attention or has an uncertain
write, failed validation, lost browser ownership, or failed account restoration.

Run-thread archiving does not pause or delete the recurring automation and does not
delete local evidence.
reproduce the failed condition without persisting post text, metrics, account
identifiers, or local paths.
