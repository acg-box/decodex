# Codex Upstream Automations

This directory is the canonical source for the standalone Codex upstream adaptation
loop. The loop does not use Decodex server, runtime intake, Linear, or tracker state.
It uses the installed `decodex` CLI only for signed commits and reviewed landing.

The active roles are:

- `codex-upstream-maintainer`: observes upstream and the installed Codex schema,
  delegates one claimed compatibility change to the trusted ephemeral-agent wrapper,
  and asks the state wrapper to validate it, commit it with `decodex commit`, and
  open a pull request.
- `codex-upstream-reviewer`: independently reviews the exact pull-request head,
  reproduces no-change decisions, requests bounded repairs, or lands it with
  `decodex land`.
- `codex-upstream-health`: checks cursor continuity, stale work, leases, and current
  Codex build evidence. It also reconciles the five exact live task definitions in
  the upstream and content manifests through the native Codex App lifecycle tool and
  verifies each readback. Its live content audit also verifies the installed xurl
  version, target OAuth2 authorization, social lineage, and X cost ledger without
  calling a paid endpoint.

All scheduled tasks run from the primary clean `main` checkout with local execution.
Maintainer and Reviewer use `gpt-5.6-sol` with `max` reasoning, Health and Content
Manager use `gpt-5.6-terra` with `high`, and Xurl Publisher uses `gpt-5.6-luna`
with `high`. No task uses `xhigh`. They are never configured with a worktree cwd.
Maintainer and Reviewer runs can create temporary task or review worktrees below
`.worktrees`. The five source definitions permit 12 scheduled task wakes per day:
four
Maintainer, two Reviewer, two Health, one Content Manager, and three Publisher.
That is 360 wakes in 30 days or 372 wakes in 31 days.
Maintainer can invoke at most four ephemeral children per day and Reviewer at most
two. The hard upper bound is therefore 18 model invocations per day,
but a child runs only after a successful claim that needs implementation or
independent review. No-op parent wakes do not create a child. Codex App does not
provide this repository with an authoritative per-invocation USD rate, so the
automation reports invocation counts instead of inventing a dollar estimate.

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
The current schema uses `state-v4.json`. Before its first start, deployment must
quiesce the old loop, prove that no unresolved external effect remains, and delete
the exact `state.json` and `state.recovery.json` artifacts. It must also delete the
five managed runtime memory files; no earlier memory is accepted as current-run
authority. Every v4 transaction acquires the old `state.lock` as a nonblocking
cutover fence. An active v3 process stops v4 before state creation, and either
legacy state file stops every v4 transaction. V4 performs no migration and has no
legacy reader.

The source state separates the latest observed upstream head, the latest head covered
by queued contiguous ranges, and the latest terminal cursor. At most 128 source ranges
can be active at once. Later observations continue queueing from the covered head, so
a long downtime cannot be truncated or overflow the state.
Repository digest drift from the installed Codex executable belongs only to
`bootstrap` and `local_build` candidates. Upstream range and release candidates keep
their own missing-method facts, but an expected digest difference from the installed
build does not force a code change. The loop does not add one rejection fixture for
each new upstream release.
Each batch records schema facts from its own terminal SHA. Release candidates bind
the tag name and resolved tag commit. A monotonic discovery sequence prevents a
schema A-to-B-to-A transition or a tag retarget from reusing an old terminal result.
The first observation queues independent main/bootstrap, stable-release, and
prerelease-release candidates, so a new installation does not wait for the next tag.
The records remain independent, but Maintainer can work only on the earliest
unresolved source candidate. A retry, review, or owned repair on that candidate
defers later source lanes. An `automation_repair` can bypass this gate so the control
plane can repair itself. This prevents duplicate workers and commits for the same
unresolved compatibility gap without discarding lane evidence.

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

Each claim also creates a separate one-time agent handoff challenge. The lease
token stays with the parent automation. A worker receipt binds the challenge to the
candidate, claim generation, original base, staged tree, and staged-path digest. An
independent Reviewer receipt binds it to the candidate, exact base/head/tree,
disposition, bounded finding codes, and complete child execution attestation. Runtime
state stores only the challenge digest and sanitized receipt provenance. Before a
child starts, state persists a prepared run bound to the exact generation, role,
base, input head, expected receipt head, and input tree. This separation lets a
repair child read the prior committed candidate while the trusted parent stages the
result on the current base. The create-only receipt completes that run. A retry
recovers the exact receipt. A candidate-and-role file lock prevents overlapping
children and stays held until the parent persists the completed receipt. A global
root lock serializes stale-run cleanup and safe removal of inactive candidate lock
files. Lock-file churn cannot consume the bounded run-root entry budget. If the
parent dies before a receipt exists, the next owner acquires that lock before it
inspects or resets the automation-owned worktree. It can retarget a prepared run to
current `main`, removes ignored residue, and reruns the state-bound context. If the
receipt was written before state persistence, the next retry recovers it before any
retarget. A completed, unconsumed run survives lease expiry and is reclaimed with
the same generation without another attempt. If its canonical receipt is missing,
the state tool refunds the recovery claim before it creates one replacement
generation only when the original execution spent an attempt. A `base_stale`
claim spends one attempt from a bounded credit. Only a completed child receipt for
that generation refunds the attempt; a child failure, block, or expired lease keeps
it spent.
These receipts are non-replayable, state-bound handoffs. They are not cryptographic
identity signatures. A prepared commit or started land effect keeps its original
handoff receipt and intent generation across lease recovery; a new owner generation
can resume only that exact intent and cannot replace its receipt.

Standalone Codex app automations do not receive native multi-agent tools. The
checked-in `run-agent` transaction therefore invokes one trusted
`codex exec --ephemeral` child. It fixes model `gpt-5.6-sol` and effort `max`,
loads neither user configuration nor execution rules, disables network, clears the
model shell environment, and sets `project_doc_max_bytes=0`. Its single Codex
sandbox starts with root read access so the runtime does not inject `:minimal`, then
removes every discovered top-level root, then reopens only trusted runtime files, a
private Git-free snapshot, a private model directory, and a private evidence
package. The candidate worktree remains denied. A real preflight probe must prove
those denials, the `/System/Volumes/Data` data root and exact protected-path aliases,
candidate-write denial from an environment-cleared new session, denied TCP and UDP
loopback sockets, and denied Keychain secret access before the model call. The
Keychain check creates a temporary fake item, proves host access, then proves that
the child cannot use SecurityServer. The final child profile also denies `security`,
`defaults`, `osascript`, Security.framework, and LocalAuthentication.framework.

The evidence package contains exact upstream patches and protocol schemas, installed
schema evidence, the target patch, and bounded diagnostics. Initial-commit evidence
omits commit metadata, and the child context uses only worktree-relative paths. The
child cannot read the full upstream mirror or the target `.git` or Git common
directory. A standalone watchdog receives only the current access and ID tokens
through a pipe. It creates a private empty-refresh-token capsule after it inherits
the candidate lock. On normal exit, timeout, signal, or parent death, it kills the
child process group plus same-user descendants bound by PID, start time, and a
per-run random supervision marker. This best-effort cleanup also removes descendants
that create a new session when the marker remains available. The marker scan is
bounded, is not written or logged, and is used only during cleanup. The tested
inherited Seatbelt profile, not descendant discovery, is the authority boundary. It
remains inherited after an environment clear or new session, so a detached
descendant still cannot write the candidate, read protected host data, or use the
network. The watchdog then deletes the capsule. Every state-tool command removes
all unlocked stale run directories before other work, including capsules left by
an uncatchable watchdog termination or power loss. The host proves that the real
auth file is unchanged. Provider keys, refresh tokens, GitHub tokens, SSH agents,
X credentials, MCP servers, plugins, browser control, lease authority, and Codex
task tools are absent.

The child returns one schema-constrained
`decodex/codex-upstream-agent-result/2` value. Maintainer output is one bounded Git
binary patch; Reviewer output has no patch. The child never writes state, a
candidate worktree, or a handoff receipt. The trusted parent verifies the exact
workspace manifest and patch digest, applies the patch to the unchanged candidate
with `git apply --check --index --binary`, permits only regular file modes, rejects
whitespace and unstaged or untracked residue, and authorizes every changed path for
the candidate kind. It denies scheduler, GitHub Actions, authentication, landing,
managed-repository, X execution, schema, and automation-control paths. Any rejected
or internally invalid applied patch is reset to the exact clean baseline. The parent
then writes the canonical create-only mode-`0600`
`decodex/codex-upstream-handoff-receipt/4` receipt. The receipt binds
the fixed model and effort, Codex version and executable digest, command and
permission digests, sandbox-probe, watchdog, workspace and evidence manifests,
prompt, schema, patch, and result digests, and start and completion times. Raw
prompts, patch text, model output, credentials, and temporary files are deleted.
`--ephemeral` creates no retained child task or rollout. Health removes canonical
handoff files that no live state generation can consume.

Only the formal land path can report `base_stale`. It must provide the pull
request's valid, changed `baseRefOid`; child output and the public repair command
cannot claim this condition. The state transition validates the exact open pull
request, old remote head, clean automation-owned branch, and current `main`. It then
atomically removes the old commit receipt and records the complete stale-refresh
target before Maintainer can claim the work. Maintainer resets only that branch to
current `main`, runs one new fenced implementation child, and updates the same pull
request with an exact force-with-lease. Reviewer and Maintainer do not consume
normal failure attempts for this external base race. Maintainer refunds its
generation-bound refresh attempt only after the completed child receipt is recorded.
A child failure, block, or lease expiry keeps the attempt spent. The loop does not
retain a legacy branch or compatibility state.

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
The trusted launcher uses Python isolated mode and disables `site` initialization for
both the version probe and the final process. Caller `PYTHONHOME`, `PYTHONPATH`,
`PYTHONUSERBASE`, user-site packages, and `sitecustomize` cannot affect either step.

On a failed validation profile, the wrapper writes one cause-addressed mode-`0600`
diagnostic under the local cache. The stable cause digest excludes output details such
as durations and temporary paths. The artifact has a separate exact digest and keeps
one SHA-256 derived from the separate stdout and stderr stream digests as local
evidence. It stores only the schema,
profile, failure code and class, repository HEAD/tree, return code, output digest,
bounded test IDs, exception classes, reason codes, and counts. It never stores raw
command output, absolute paths, credentials, email addresses, or private prose. The
failed command returns the stable cause digest as its `error_digest`. The named
artifact is the unambiguous local lookup for that cause.
If candidate output contamination or cleanup also fails, the command keeps the
profile failure as `error_code` and returns the additional bounded reason in
`related_error_codes`.
Maintainer and Health read it only through
`run_upstream_autopilot validation-diagnostic --error-digest <digest> --json`, which
revalidates the cause identity and the separate artifact digest. Maintainer passes
the returned bounded structure to its worker. It does not pass a primary-checkout
cache path into the candidate worktree.

The diagnostics directory is mode `0700`. It uses descriptor-relative, no-follow
file operations and a process lock. Diagnostic files must be owned by the current
UID, have one link, and have exact mode `0600`. The store keeps at most 512 files and
8 MiB. Pruning does not remove a digest that a nonterminal candidate references. If
state cannot be read or active references consume the capacity, the write fails
closed.

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
cargo build --locked -p decodex-publisher
python3 automations/decodex/scripts/config/evaluate_automations.py \
  --manifest automations/upstream/automations.toml
python3 automations/decodex/scripts/config/evaluate_automations.py \
  --manifest automations/decodex/automations.toml
python3 automations/decodex/scripts/config/render_automation_plan.py --json
```

The plan command renders native lifecycle inputs for this manifest and the current
two-task content manifest. It is read-only. Live task creation and changes in Codex
Desktop use the native automation lifecycle tool. Health can create or repair only
the five fixed IDs in those manifests. It must view an existing ID before an update
and read back each mutation. The plan retires exactly
`decodex-x-browser-publisher`; Health removes that ID when present and never
recreates it. Health never lists, edits, or deletes unrelated tasks and never
writes scheduler files directly.
Codex App alone owns `created_at` and `updated_at`. The live evaluator rejects
missing or invalid list timestamps.
Live xurl readiness never executes xurl from Python. Health builds the current
Publisher. Before its probe, Health runs
`run_upstream_autopilot x-pricing-audit --json`. The audit makes one ordinary HTTPS
GET only to the pinned official Markdown URL. It parses one unique row for Post
Read, User Read, URL-free Post Create, and Post Create with URL. It stores no page.
It atomically renews a mode-`0600` receipt with the URL, parser version, fetch time,
raw digest, and integer micro-USD rates. The receipt is valid for at most 36 hours.
The Publisher requires exact rates of 5,000, 10,000, 15,000, and 200,000
micro-USD, respectively, and the 1,250,000 micro-USD monthly cap.

An exact rate change or a parser failure after a valid receipt queues one critical
`x_pricing_contract_drift` candidate with a bounded receipt projection. A queued
candidate takes newer audited evidence before work starts. An in-progress candidate
gets a successor for newer evidence. Maintainer updates constants and fixtures only
from that projection. Reviewer checks the same receipt binding before landing.
Network failure preserves the prior receipt only until its 36-hour limit. Missing,
stale, future, malformed, tampered, or mismatched receipts stop publication.
Each parse failure also writes a content-addressed private receipt. Candidate state
binds that exact digest, so a later failure or successful audit cannot replace the
evidence under review. Cleanup preserves every referenced receipt, keeps at most 64
unreferenced receipts, and reserves one incoming receipt beyond 512 retained files.
The hard limit is 513 files and 8,404,992 bytes.

Both Health and the live evaluator invoke only
`decodex-publisher social probe-xurl`. They consume its bounded JSON readiness
report. The Rust entrypoint owns exact version and binary binding, OAuth-status
calls, the non-secret least-privilege authorization contract, and pricing-policy
freshness.
Repo-only evaluation remains static and starts no process.
Health also runs `decodex-publisher social cost-report`; Publisher is the sole v4
ledger parser and returns only bounded monthly ceilings and call counts.
Health also queues a bounded `content_loop_degraded` repair when validated content
evidence misses its freshness, publication, outcome, or account-restoration service
level. The candidate stores only bounded degradation codes, so Maintainer can
reproduce and repair the fault without reading social content.

## Scheduled Run Tasks

Maintainer, Reviewer, Health, Content Manager, and Publisher apply the shared
`scheduled-run-thread-retention.md` policy. After all durable and external-effect
readbacks, a terminal role creates one mode-`0600` owner receipt with
`task-retention-seal`. The receipt is keyed by the app-provided `CODEX_THREAD_ID`
and contains only the automation ID, task ID, allowlisted terminal result code,
nullable evidence kind, digest of validated evidence bytes, timestamp, and status.
It contains no evidence path, task text, personal data, raw response, local path,
rollout, or Codex database data.
An evidence-bearing seal also runs the current owned Publisher binary's canonical
full-store `validate-social` command. It rereads the evidence afterward and
requires exact byte equality before writing the receipt.

`task-retention-plan` scans only this bounded receipt directory, excludes the active
Health task, and returns at most 50 pending task records bound to owner, result,
evidence kind, and evidence digest. Health calls native `read_thread` and
`set_thread_archived` directly for each exact ID. Python never calls native task
tools and does not use `list_threads`. Health records
`archived_readback_confirmed` only after an exact post-archive native read. Failed,
blocked, cancelled, needs-attention, user-continued, ambiguous, human-only, or
incompletely read-back tasks settle as `keep_visible:<reason>`. The manager retains
at most 128 settled receipts for at most 30 days. Pending receipts are not removed
by age.

Task archiving does not pause or delete the recurring automation and does not
delete local evidence.

Before its first social validation, Health runs Publisher social GC. GC recovers any
durable deletion journal under the shared mutation lock before it scans or plans new
deletion. A recovery conflict fails closed. Automation memory uses only the fixed
`decodex/automation-memory/1` title and typed field allowlist. It contains bounded
reason codes, counts, opaque IDs, SHA values, and micro-USD ceilings only; it never
contains prompts, task or post text, raw responses, personal data, or local paths.
Live evaluation rejects memory that is not an owner-only, mode-`0600`, regular
non-symlink file of at most 4 KiB. Maintainer and Reviewer do not read or write
memory; their runtime memory files must be absent.
