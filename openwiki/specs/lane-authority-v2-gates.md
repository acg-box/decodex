---
type: "Reference"
title: "Lane Authority V2 Gate Manifest"
openwiki_generated: true
---

# Lane Authority V2 Gate Manifest

Status: superseded and frozen by the [vNext authority decision](../decisions/vnext-authority.md).
C1-C7 are canceled as implementation gates. The commands and scenarios below are
retained only as historical design, review, and incident evidence.

This file makes C0-C7 advancement falsifiable. The machine scenario manifest lives at
`apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/scenario_manifest.json`
and maps every scenario id to one exact future Rust test name and checkpoint at C0. The C0
baseline verifier compares the parsed table to an independently frozen, domain-separated
scenario-set digest and count over id, checkpoint, test name, scenario text, and required
result, then fails on missing, duplicate, renamed, semantically changed, wrong-checkpoint,
or unexpected rows. Changing that freeze constant is a reviewed scope change, not a
manifest regeneration step. From C1 onward the gate verifier also runs
`cargo test -- --list`, fails on missing/duplicate/skipped/unexpected tests, then runs each
required implemented test by exact name and records its result. Each checkpoint records
command exit codes, exact commit and PR head, fixture SHA-256 digests, and assertions in
[Lane Authority v2 checkpoints](../evidence/lane-authority-v2-checkpoints.md).

No checkpoint advances with an unresolved blocker/high authority objection, a skipped
required scenario, an unrecorded fixture digest, or a command whose output is too weak to
prove the required assertion.

## Stable Fixtures

| Fixture | Required path | First gate |
| --- | --- | --- |
| scenario-to-test manifest | `apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/scenario_manifest.json` | C0 |
| closed-world launcher candidate inventory | `apps/decodex/src/bootstrap/tests/fixtures/lane_authority_v2/launcher_inventory.json` | C0 |
| closed-world legacy source-node/read/write/discovery inventory | `apps/decodex/src/state/tests/fixtures/lane_authority_v2/legacy_authority_inventory.json` | C0 |
| v12 global-key collisions and partial overwrite | `apps/decodex/src/state/tests/fixtures/lane_authority_v2/schema_v12_collisions.json` | C1 |
| PUB-1711 wrong-project admission | `apps/decodex/src/program_intake/tests/fixtures/lane_authority_v2/pub_1711_wrong_project.json` | C2 |
| effect crash and provider readback matrix | `apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/effect_replay.json` | C1 |
| machine mutation/effect-kind registry | `apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/mutation_registry.json` | C0 |
| current-Linear degraded capability/tool surface | `apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/linear_capability_surface.json` | C3 |
| PUB-1704/#826 to PUB-1705/#827 supersession | `apps/decodex/src/recovery/tests/fixtures/lane_authority_v2/pub_1704_superseded.json` | C4 |
| authority projection privacy corpus | `apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/privacy_corpus.json` | C1 |
| no-effective-delta and manual landing CLI cases | `apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/adjacent_defects.json` | C6 |

Fixtures contain public or synthetic data only. The checkpoint ledger records their
SHA-256 digests after creation; changing a digest requires an explicit scope-change entry
and fresh skeptic review.

## C0 Architecture Freeze

Required scenarios: architecture requirements represented by all IDs in the target
contract plus the three frozen baseline inventories. Runtime implementation fixtures are
not required yet, but implementation cannot begin against an unbounded source baseline.

Commands:

```sh
cargo make check
scripts/verify_lane_authority_v2_baseline.sh
git diff --check
test "$(git rev-list --count HEAD..origin/main)" -eq 0
for file in \
  openwiki/decisions/lane-authority-v2.md \
  openwiki/specs/lane-authority-v2.md \
  openwiki/specs/lane-authority-v2-effects.md \
  openwiki/specs/lane-authority-v2-gates.md \
  openwiki/evidence/lane-authority-v2-checkpoints.md \
  scripts/lane_authority_v2_baseline.py \
  scripts/verify_lane_authority_v2_baseline.sh \
  apps/decodex/src/bootstrap/tests/fixtures/lane_authority_v2/launcher_inventory.json \
  apps/decodex/src/state/tests/fixtures/lane_authority_v2/legacy_authority_inventory.json \
  apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/mutation_registry.json \
  apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/scenario_manifest.json; do
  exit_code=0
  diagnostics="$(git diff --no-index --check /dev/null "$file" 2>&1)" || exit_code=$?
  test "$exit_code" -eq 1
  test -z "$diagnostics"
done
bad_file="$(mktemp)"
trap 'rm -f "$bad_file"' EXIT
printf 'trailing whitespace \n' > "$bad_file"
exit_code=0
diagnostics="$(git diff --no-index --check /dev/null "$bad_file" 2>&1)" || exit_code=$?
test "$exit_code" -ne 1
test -n "$diagnostics"
rm -f "$bad_file"
trap - EXIT
```

Expected assertions:

- the repository-owned broad gate and all whitespace checks report no failure;
- launcher, mutation, and legacy source-node inventories cover every tracked repository
  Rust/Python/Swift/shell/TOML/YAML file, including root/package build manifests, at exact
  current main with stable digests. Every regex hit is explicitly
  `unclassified_pending_c1i`, never a supported launcher or authority claim; outside explicit
  dependency/build-output components, ignored as well as ordinary untracked source/config
  paths fail verification, and a disposable-repository negative control proves the ignored
  path cannot escape. The same fixture force-tracks a file under an ignored build component
  and proves tracked files are never excluded because of their directory name;
- candidate precision controls reject bare UI `Decodex`/`Lane`, ordinary `.update(...)`,
  and YAML `pull_request:` while retaining structural process, SQL, LaneId, and provider
  readback examples. C1I remains responsible for final AST/syntax/call-graph classification;
- the scenario table matches the independent C0 count/digest freeze, and in-memory
  checkpoint plus required-result mutation controls prove regeneration cannot bless
  scenario drift;
- mutation registry expands every normative effect row to one concrete `effect_kinds`
  entry with complete semantics, matches an independent count/digest freeze over every
  semantic field, rejects a desired-state-readback mutation in a negative control, and
  separately maps frozen v12 source candidates;
- no C0 assertion depends on an OpenWiki-specific Decodex CLI/MCP/runtime surface;
- branch contains current origin/main, and the ledger classifies every intervening
  launcher/mutation surface before review;
- XY-1251 links XY-1248, XY-1249, and XY-1250;
- the checkpoint ledger contains current main, #1073 head/disposition, schema facts,
  first and subsequent skeptic verdicts, and unresolved objections;
- a fresh final C0 skeptic returns `READY` with no blocker/high authority gap; and
- the exact branch is committed, pushed, and linked to XY-1251 before C1 starts.

## C1 Identity And Migration

Required scenarios: ID-01..16, MIG-01..17, MIG-20, MIG-23, MIG-25, QUA-01..07,
EFX-01..03, EFX-14, EFX-18..20, EFX-22, EFX-24, EFX-27, EFX-30..32,
EFX-34, EFX-36..37, and TEL-04..09.

C1I, C1A and C1B are ordered subgates with separate commits/PRs and evidence. C1I may
add only inventory parsers/verifiers and fixture refinements; it must run against the
frozen C0 source nodes and classify every language AST/syntax/call-graph site before any
runtime behavior changes. C1A contains only the guard/supervisor rollout. Before C1A,
host migration refuses because guard
coverage is absent. C1A is not complete until the deployed binary, shims, desktop app,
daemon, MCP, and automation launcher identities are read back and every supported launch
proves it acquires the generation lock. C1B contains dormant v2 foundations and
migration tooling; host apply remains disabled until the exact C7 activation release.
C1B cannot begin from merely merged-but-not-deployed C1A code.

Commands for C1I:

```sh
scripts/verify_lane_authority_v2_baseline.sh --post-c0
scripts/verify_lane_authority_v2_legacy_authority.sh --checkpoint C1I
scripts/verify_lane_authority_v2_mutations.sh --checkpoint C1I
scripts/verify_lane_authority_v2_launchers.sh --inventory-only
scripts/verify_lane_authority_v2_gates.sh C1I
```

`--post-c0` disables only the initial C0 changed-path allowlist after C0 has landed. It
still requires the frozen baseline ancestor and byte-exact source, effect, and scenario
manifests; C0 itself must run the command without that flag.

Commands for C1A:

```sh
cargo test -p decodex generation_guard --all-features --quiet
scripts/verify_lane_authority_v2_launchers.sh --all
decodex maintenance generation-guard-audit --json
scripts/verify_lane_authority_v2_gates.sh C1A
```

Commands for C1B:

```sh
cargo test -p decodex lane_authority_v2_c1 --all-features --quiet
cargo test -p decodex migration_v13 --all-features --quiet
cargo test -p decodex effect_protocol_core --all-features --quiet
cargo test -p decodex authority_output_boundary --all-features --quiet
cargo check --all-features --all-targets --workspace
scripts/verify_lane_authority_v2_gates.sh C1
scripts/verify_lane_authority_v2_legacy_authority.sh --checkpoint C1
scripts/verify_lane_authority_v2_mutations.sh --checkpoint C1
scripts/verify_lane_authority_v2_output_sinks.sh
scripts/verify_lane_authority_v2_gates.sh C1B
```

Expected assertions:

- C1I proves every frozen source file and high-recall candidate maps to parsed
  Rust/Python/Swift/shell syntax/call graph or TOML/YAML semantic document, every
  discovered production mutation/launcher has an exact registry owner/disposition, and
  no runtime behavior changed;
- C1A-before/after tests prove migration refusal before guard deployment, lock ownership
  for every supported launcher after deployment, and refusal while any old process or
  open handle remains; exact deployed identities are recorded before C1B, including the
  primary-main-checkout automation resolver, manifest sync, evaluator/manager/review jobs,
  and live-config writer added through `4566948f`;
- focused tests execute every required scenario id and pass;
- isolated-fixture dry-run/apply classifier output is byte-equivalent except apply
  receipts/timestamps; host apply remains disabled;
- migration lock, immutable backup, every cutover and rollback filesystem crash point,
  SQLite-exclusive v12 directory detach/tombstone, manifest generation, local-effect
  PONR, rollback-status, connected quarantine, manual closeout receipt disposition, and
  old-binary race/refusal are asserted;
- typed migration edges prove project roots do not spread quarantine, explicit Program
  ExecutionGroups do, and every source node has exactly one typed partition;
- ambiguous project roots create ProjectBindingQuarantine without a ProjectKey and
  cannot route or infer identity from paths/latest rows;
- every project-contract/DB write and rollback crash point preserves ProjectKey and
  generation agreement using the immutable database/contract backup bundle;
- rollback decrypts to 0600, verifies digest, applies/fsyncs each recorded uid/gid/mode
  before rename, and round-trips both 0600/0644 contract fixtures;
- every other inventoried legacy artifact remains byte/mode/path identical pre-PONR and
  rollback verifies the complete planned hash inventory; legacy agent evidence instead
  follows journaled encrypted-vault sealing/restoration and is undiscoverable normally;
- macOS Keychain and Linux Secret Service KeyProtector crash matrices verify age-v1
  bundle/key persistence, in-memory SQLite backup/zeroization, no named plaintext backup,
  planned rollback temps only, and fail-closed unsupported hosts;
- normal registration/revision publication stays pending and unroutable until DB and
  contract attestations agree; migration plan/dry-run/apply reuse one fsynced ProjectKey
  allocation and classifier digest;
- contract content fingerprint and final-file digest use a fixed non-self-referential
  preimage and round-trip exact bytes;
- ProjectAvailability pause/resume/retire and finite routing-predicate intersection are
  epoch-fenced, globally checked, history-preserving, and reject cascade deletion;
- RoutingCatalog CAS serializes registration/revision/pause/resume/retire/adjudication;
  ProjectAlias/HostCheckout changes do not alter semantic binding fingerprints; first
  registration creates paused availability epoch 1 atomically;
- overlapping paused predicates are allowed and only resume/active migration runs active
  intersection; pause rebases non-invoking operations to convergence-only and fences
  stale workers;
- lane operations seal semantic RoutingAttestation rather than global catalog epoch;
  unrelated catalog changes reattest by CAS, while drifted unknown effects read back and
  classify without forward replay;
- ProjectBinding/LaneId, sole `commit_transition` writer, effect store/uniqueness,
  claimant fencing, unknown reconciliation, and PONR protocol are available before
  intake starts using v2 effects;
- one AuthorityTransaction atomically owns epoch/claim CAS, operation/effects,
  active-operation pointer, authority event, Lane transition, and Program/intake rows;
  statement-level failure injection leaves no partial authority;
- finalize_operation atomically finalizes all effects, Lane/resources/event, active
  pointer, and operation state; receipt-bound intake continuation never mutates an
  existing effect plan or leaves an unowned published issue;
- rebind and quarantine adjudication prove fresh exactly-one routing, immutable
  identities, no active operation, epoch CAS, atomic reservation/claim updates, and
  distinct accountable principals; project quarantine resolve/split maps every source
  node exactly once before creating globally unique ProjectKeys;
- unbound zero/multi-match routing persists rejection/quarantine occupancy and candidates
  without fabricated ProjectKey/Lane/Program; project split keeps quarantine through all
  pending contract effects and batch activation;
- central typed OutputBoundary, panic/error/provider sinks, privacy corpus, TEL-04, and
  direct-output scan pass before any v2 mutation path can ship; agent evidence uses its
  exact private schema, and sealed transport-derived InvocationIdentity rejects caller
  attribution forgery;
- subject-specific expected versions make registration/project-quarantine/Lane/migration
  operations constructible without fabricated epochs; supervisor broker credentials
  reject direct/replayed/altered binaries and same-root dual accountability;
- local Git/filesystem/process/hook plans seal HostCheckoutAttestation resource/epoch and
  reject relocated/stale checkout targets;
- ExecutionGroup has epoch/state/terminal prerequisites; transfer IntakeAuthority keeps
  source provenance with fresh destination attestation, source node/group disposition,
  and atomic destination group/mapping creation;
- the reverse scan over v2 modules returns no legacy authority API or schema use; the
  runtime-format selector test proves v2 mutation is unreachable while v12 remains the
  sole active host authority; and
- the legacy inventory verifier covers every v12 table, issue-only lease/worktree API,
  lifecycle reader/writer, marker reader/writer/path discoverer, direct SQL authority
  read/write, and proves all v2 Lane writes dominate through
  `LaneStore::commit_transition`; unknown schema/artifacts and reader-only sources fail;
- AST/call-graph and sealed-capability verification covers existing fetch,
  default-branch ref/index/worktree updates, SQLite, provider/process/hook, runtime
  config, evidence/maintenance, and filesystem calls, and rejects every unregistered/
  string-built mutation path;
- C1 permits only exact machine-inventoried `v12_legacy` callsites with replacement kind
  and C7 removal checkpoint, while an ordinary v12 lifecycle test proves no v2 row is
  used; v2 callsites are capability-bound and C7 requires the legacy set empty;
- effect state CAS, receipt and typed telemetry event commit atomically with sequence
  uniqueness and statement-level failure injection;
- lane-attempt worker process callsites are registered and legacy issue-claim/
  dispatch-lock readers/writers/artifacts are eliminated as authority;
- fixture digest and a sanitized dry-run classification summary are recorded.

## C2 Intake And Dispatch

Required scenarios: ID-17, ADM-01..09 and EFX-17.

Commands:

```sh
cargo test -p decodex lane_authority_v2_c2 --all-features --quiet
cargo test -p decodex program_intake --all-features --quiet
cargo test -p decodex dispatch_policy --all-features --quiet
cargo test -p decodex linear_issue_effects --all-features --quiet
scripts/verify_lane_authority_v2_gates.sh C2
```

Expected assertions:

- wrong/zero/overlapping binding and ledger-write failure leave no Program, mapping,
  claim, lease, or worktree row;
- workspace-qualified issue resolution obtains immutable provider key before ProjectKey;
  bare/cross-workspace identifiers and caller-selected project/token lookup reject;
- fully paginated double-pass team/label/version snapshots reject incomplete or torn
  routing facts, including forbidden labels beyond page one;
- current Linear goal-create and conditional update/archive effects reject before
  invocation because provider idempotency/CAS capabilities are absent; issue-batch
  intake remains supported;
- a provider-capable fixture proves immutable create idempotency, receipt-bound
  continuation, occupancy-collision quarantine, and conditional cleanup without an
  unowned executable tracker issue;
- archive hygiene cannot bypass operation fencing or fall back to stale read-then-write;
- accepted issue-batch authority remains valid without a Decision Contract;
- PUB-1711 replay records origin, principal/job, selector, candidate evaluations,
  resolver version, rejection reason, and correlation propagation; and
- fixture digest plus before/after row counts are recorded.

## C3 Transition And Effects

Required scenarios: EFX-04..13, EFX-15..16, EFX-21, EFX-23, EFX-25..26,
EFX-28..29, EFX-33, and EFX-35.

Commands:

```sh
cargo test -p decodex lane_authority_v2_c3 --all-features --quiet
cargo test -p decodex effect_reconciliation --all-features --quiet
scripts/verify_lane_authority_v2_mutations.sh
scripts/verify_lane_authority_v2_linear_surfaces.sh
scripts/verify_lane_authority_v2_gates.sh C3
cargo make test
```

Expected assertions:

- every remaining registered mutation kind, legal effect transition, and crash edge is
  exercised on top of the C1 core;
- stale claimant, process-generation lock, all-page marker, ordinal barrier,
  compensation failure, revalidation before every invocation class, and PONR refusal
  are asserted;
- new-ref push followed by PR-create failure remains durable; separate leased cleanup
  and lease-failure roll-forward never report compensation;
  update-ref push proves `published_pending_handoff` without automatic rewind;
- PR creation remains a durable publication, delivered interrupt/terminate never claim
  compensation, and cleanup resumes remote-ref, worktree, local-ref destructive order;
- remote-config changes use exact prior/new digest CAS; soft interrupt/steer requests
  use accepted-before-act request ids and reconcile consumed-without-response crashes
  without redelivery;
- current Linear create/update/archive effects are capability-unsupported and stop
  before invocation; capable-provider fixtures prove immutable create idempotency and
  conditional mutation CAS under races;
- a complete current-Linear issue-batch lane uses internal authority/GitHub/local cleanup
  without any unsupported call; optional append-only comment debt cannot block it;
- CLI `--help`, MCP schemas, dynamic agent tools, tracker-tool dispatch, scheduler and
  closeout call graphs expose no current-Linear create/state/brief/label/relation/archive
  mutation or queue-label polling capability; only read/snapshot and optional append-only
  comment surfaces match the machine manifest;
- fetch writes only operation-scoped refs with `--no-write-fetch-head`, and credential
  helper publish/retire proves exact owner, digest, mode, process, and expiry fencing;
- remote-ref creation stays durable; Git deletion requires server lease, unsupported
  GitHub expected-OID mutation stops, and lane workers are exact binary/PID/group fenced;
- account login uses supervised exact Codex process plus private 0700 temp-home/auth-import/
  exact-retire effects, exposes no secret path/payload, and leaves no preserved workspace;
- provider capability probes and exact supported/unsupported mutation kinds are recorded;
- machine registry verification reports zero unregistered/direct mutation call sites;
  and
- fixture digest and crash-point coverage count are recorded.

## C4 Supersession And Conflict Release

Required scenarios: SUP-01..19.

Commands:

```sh
cargo test -p decodex lane_authority_v2_c4 --all-features --quiet
cargo test -p decodex supersession --all-features --quiet
cargo test -p decodex conflict --all-features --quiet
scripts/verify_lane_authority_v2_gates.sh C4
```

Expected assertions:

- staged handoff and acceptance remain immutable and distinct;
- predecessor epoch and unique active handoff/terminal edge CAS permit one winner;
  replacement/cancellation/stale losers remain immutable and release nothing;
- every predecessor patch has a typed disposition;
- the canonical PatchUnit disposition universe has exactly one unit per endpoint path
  delta, empty commit, and merge topology, with no duplicate/evidence-only units;
- multiple best merge bases reject; unique-base commit ordering is parents-first Kahn
  traversal with raw-OID tie breaks and stable bytes;
- endpoint-net-zero path history emits one deterministic ordered transition unit and one
  disposition;
- terminal authority and conflict release commit together;
- the deterministic superseded-closeout operation survives crash/retry around every
  acceptance, local terminalization, PR readback/close, cleanup, and projection boundary
  without duplicate effect or reconstructed lineage;
- failed cleanup remains non-executable with only fenced cleanup ownership;
- the PUB-1704/PUB-1705 fixture releases the obsolete conflict without using #1073
  production code; and
- replacement dry-run JSON, fixture digest, and exact replacement PR head are recorded.

## C5 Telemetry And Privacy

Required scenarios: TEL-01..03 and TEL-10; C1's TEL-04..09 remain mandatory and are rerun.

Commands:

```sh
cargo test -p decodex lane_authority_v2_c5 --all-features --quiet
cargo test -p decodex authority_projection_privacy --all-features --quiet
cargo test -p decodex authority_operator_readback --all-features --quiet
scripts/verify_lane_authority_v2_output_sinks.sh
scripts/verify_lane_authority_v2_gates.sh C5
```

Expected assertions:

- typed deny-by-default serializers cover admin/operate/observe MCP, CLI text/JSON,
  dashboard, log, metric, error/crash, agent evidence/forensic receipt, migration,
  checkpoint, Linear, and GitHub schemas;
- unknown and forbidden fields fail serialization tests;
- secret/path/provider-body/protocol markers are absent from every typed output sink and
  the direct-output scan is empty outside allowlisted sink modules;
- PUB-1711 replay timeline answers who/what selected the project and why; and
- signed hash-chain audit detects rewrite/delete/truncate/reorder/fork while recovering
  only the valid DB-ahead/protected-head-behind crash window;
- privacy fixture digest plus positive/negative assertion counts are recorded.

## C6 Adjacent Defects

Required scenarios: ADJ-01..04.

Commands:

```sh
cargo test -p decodex lane_authority_v2_c6 --all-features --quiet
cargo test -p decodex no_effective_delta --all-features --quiet
cargo test -p decodex manual_authority --all-features --quiet
scripts/verify_lane_authority_v2_gates.sh C6
```

Expected assertions:

- unexpected no-effective-delta schedules exactly one deterministic retry with exact
  base/head/PatchSet/name-only/status/expected-surface/acceptance diagnostics; a repeated
  result converges to reason-coded attention, while independently proven
  `already_satisfied` is a distinct success decision;
- explicit blocked evidence does not become a false no-op retry; and
- every `--manual-authority --related` combination is rejected by Clap before repository
  or provider readback, and related fields are absent from manual request/builders.

## C7 Final Integration And Cleanup

Required scenarios: every ID/QUA/MIG/ADM/EFX/SUP/TEL/ADJ scenario.

Commands:

```sh
cargo make check
git diff --check main...HEAD
/Users/x/.codex/shims/codex-identity
export GH_HOST=github.com
test -n "$DECODEX_C7_PR"
case "$DECODEX_C7_PR" in
  https://github.com/hack-ink/decodex/pull/*) ;;
  *) exit 1 ;;
esac
pr_number="${DECODEX_C7_PR##*/}"
case "$pr_number" in
  ""|*[!0-9]*) exit 1 ;;
esac
test "$DECODEX_C7_PR" = "https://github.com/hack-ink/decodex/pull/$pr_number"
test -x "$DECODEX_C7_BINARY"
test -n "$DECODEX_LANE_AUTHORITY_V2_PLAN"
test -n "$DECODEX_LANE_AUTHORITY_V2_CUTOVER_RECEIPT"
pr_head_sha="$(gh pr view "$DECODEX_C7_PR" --repo github.com/hack-ink/decodex \
  --json headRefOid --jq .headRefOid)"
test "$pr_head_sha" = "$(git rev-parse HEAD)"
scripts/verify_lane_authority_v2_required_checks.sh \
  --repository https://github.com/hack-ink/decodex \
  --commit "$pr_head_sha" --phase pull-request
# Fresh code and skeptic reviews occur here, then Decodex lands the exact PR head.
merge_commit_sha="$(gh pr view "$DECODEX_C7_PR" --repo github.com/hack-ink/decodex \
  --json mergeCommit --jq .mergeCommit.oid)"
test -n "$merge_commit_sha"
remote_main_sha="$(gh api --hostname github.com \
  repos/hack-ink/decodex/git/ref/heads/main --jq .object.sha)"
test "$remote_main_sha" = "$merge_commit_sha"
scripts/verify_lane_authority_v2_required_checks.sh \
  --repository https://github.com/hack-ink/decodex \
  --commit "$merge_commit_sha" --phase main-activation
attestation_json="$(mktemp)"
trap 'rm -f "$attestation_json"' EXIT
gh attestation verify "$DECODEX_C7_BINARY" \
  --repo hack-ink/decodex \
  --signer-workflow github.com/hack-ink/decodex/.github/workflows/lane-authority-v2-activation.yml \
  --source-ref refs/heads/main \
  --source-digest "$merge_commit_sha" \
  --deny-self-hosted-runners --format=json > "$attestation_json"
scripts/verify_lane_authority_v2_activation_provenance.sh \
  --binary "$DECODEX_C7_BINARY" \
  --attestation "$attestation_json" \
  --pr "$DECODEX_C7_PR"
test "$("$DECODEX_C7_BINARY" build-info --json | jq -r .source_commit)" = \
  "$merge_commit_sha"
test "$("$DECODEX_C7_BINARY" build-info --json | jq -r .dirty)" = false
decodex supervisor cutover-prepare \
  --binary "$DECODEX_C7_BINARY" \
  --pr "$DECODEX_C7_PR" \
  --verified-attestation "$attestation_json" \
  --require-format v12 --drain --stop \
  --plan-output "$DECODEX_LANE_AUTHORITY_V2_PLAN" \
  --receipt-output "$DECODEX_LANE_AUTHORITY_V2_CUTOVER_RECEIPT" --json
"$DECODEX_C7_BINARY" maintenance lane-authority-v2 dry-run \
  --cutover-receipt "$DECODEX_LANE_AUTHORITY_V2_CUTOVER_RECEIPT" \
  --plan "$DECODEX_LANE_AUTHORITY_V2_PLAN" --json
"$DECODEX_C7_BINARY" maintenance lane-authority-v2 apply \
  --cutover-receipt "$DECODEX_LANE_AUTHORITY_V2_CUTOVER_RECEIPT" \
  --plan "$DECODEX_LANE_AUTHORITY_V2_PLAN" --json
"$DECODEX_C7_BINARY" supervisor cutover-preflight \
  --cutover-receipt "$DECODEX_LANE_AUTHORITY_V2_CUTOVER_RECEIPT" \
  --require-format v2 --kernel-ipc-probe --disable-external-effects \
  --disable-non-probe-process-spawn \
  --rollbackable-writer-probe --json
"$DECODEX_C7_BINARY" maintenance lane-authority-v2 commit-point-of-no-return \
  --cutover-receipt "$DECODEX_LANE_AUTHORITY_V2_CUTOVER_RECEIPT" \
  --json
"$DECODEX_C7_BINARY" supervisor cutover-activate \
  --cutover-receipt "$DECODEX_LANE_AUTHORITY_V2_CUTOVER_RECEIPT" \
  --require-format v2 --json
"$DECODEX_C7_BINARY" maintenance lane-authority-v2 status --json
"$DECODEX_C7_BINARY" maintenance lane-authority-v2 rollback-status --json
"$DECODEX_C7_BINARY" maintenance generation-guard-audit --json
"$DECODEX_C7_BINARY" maintenance authority-audit --all --json
"$DECODEX_C7_BINARY" status --json --limit 20
scripts/verify_lane_authority_v2_gates.sh all
scripts/verify_lane_authority_v2_legacy_authority.sh --checkpoint C7
scripts/verify_lane_authority_v2_mutations.sh
scripts/verify_lane_authority_v2_output_sinks.sh
scripts/verify_lane_authority_v2_linear_surfaces.sh
rm -f "$attestation_json"
trap - EXIT
```

Expected assertions:

- repository gate, exact-head CI, code review, and fresh skeptic review pass;
- identity, exact reviewed PR-head checks, landed merge commit, live remote/main,
  main-activation required checks, OIDC-signed artifact provenance, pinned trusted-builder
  workflow, embedded binary build-info and computed binary SHA-256 agree immediately
  before cutover-prepare without operator-supplied source/tested hashes; all identities,
  check-run ids and attestation digest are signed into the cutover receipt; PONR rechecks
  live main/source binding and fails if main advanced;
- `cutover-prepare` derives artifact digest, source commit, tested PR head, workflow
  identity, and required-check ids only from verified attestation, binary build-info, and
  fresh GitHub readback. Later stages accept only the signed receipt and rederive/recheck
  live facts; no CLI flag can inject those authority identities. The sole PR locator is an
  exact canonical `hack-ink/decodex` URL, every GitHub readback also pins that repository,
  and ambient checkout, `origin`, `GH_HOST`, or `GH_REPO` state cannot redirect it. The
  required-check helper rejects any repository except the canonical GitHub URL;
- authority audit returns zero executable ambiguity, orphan claim, legacy authority
  writer, terminal conflict lease, stale active operation, and overdue unknown effect;
- the final full-source reverse scan returns no legacy authority reader/writer or schema
  declaration outside the exact allowlisted read-only offline migration decoder;
- C1-C6 artifacts are proven dormant on v12, the exact C7 binary is pinned before
  migration, live cutover selects v2 once, and post-cutover launch rejects v12 runtime;
- cutover-prepare evidence proves every lane drained, supervisor stopped, writer lock
  exclusive, SQLite legacy transaction exclusive through v12 directory detach/tombstone,
  and one-use receipt bound to v12 generation plus exact attested binary SHA-256;
  Ed25519 HostAuthorityKey/KeyProtector, deterministic-CBOR receipt and fsynced stage
  journal reject forged/copied/truncated/reordered/replayed/expired receipts and key
  rotation during a session;
  exact-binary preflight exercises normal v2 open and the complete production
  `decodex.authority-broker/1` framing, method authorization, request dedupe, fsync/ack,
  resume and commit-transition path over real kernel IPC/peer/FD credentials, plus negative
  replay/hash/peer/sequence probes, output boundary, startup invariants and a rollbackable
  writer probe; all probe children are reaped and external/non-probe process adapters
  remain disabled, so failure leaves PONR absent and rollback available;
  explicit PONR is then fsynced before restart, cutover-activate consumes that receipt and
  restarts only the pinned binary, and every exact-binary status/generation readback
  proves v2 manifest/database/contracts while rollback-status refuses restoration;
- live migration readback proves every project contract and v2 ProjectBinding shares the
  recorded ProjectKey/generation and the immutable encrypted database/contract/evidence
  backup bundle remains verified;
- legacy evidence vault/ciphertext/KeyProtector handle is verified on the host, raw
  evidence is absent after cutover, and no normal v2 discovery/readback exposes vault
  plaintext;
- all replacement PRs land through Decodex and #1073 is closed as superseded;
- XY-1248/1249/1250/1251 reach their evidence-supported terminal states; and
- all goal-created worktrees are removed and the pre-existing inventory is classified
  without deleting unrelated user-owned worktrees.
