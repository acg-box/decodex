# Commands And Validation

Use this page when deciding which command validates a change. It summarizes current task-runner authority and maps test families to source areas.

## Task runner authority

`Makefile.toml` owns repository task names. The broad readiness gate is:

```sh
cargo make check
```

It depends on build, node checks, Rust checks, formatting, lint, and tests (`Makefile.toml`). Use it before claiming broad readiness. For documentation-only or narrow source changes, run the smallest relevant checks and state the narrowed scope.

## Main gates

| Purpose | Command |
| --- | --- |
| Broad repo check | `cargo make check` |
| Rust type check | `cargo make check-rust` or `cargo check --all-features --all-targets --workspace` |
| Rust tests | `cargo make test` or `cargo nextest run --workspace --all-targets --all-features` |
| XY-1306 path/config/blob/cache foundation | `cargo test -p decodex-core --all-targets --all-features` |
| XY-1307 daemon bootstrap and doctor protocol | `cargo test -p decodex-core -p decodex-protocol -p decodex-postgres -p decodex-runtime -p decodexd --all-targets --all-features` |
| XY-1308 API-only CLI and diagnostic matrix | `cargo make test-vnext-cli-diagnostics` |
| vNext dependency architecture | `cargo make test-vnext-architecture` |
| vNext PostgreSQL store, Conversation history, blobs, and Context Packs | `cargo make test-vnext-postgres-store` |
| vNext storage feasibility proof | `cargo make test-vnext-storage-proof` |
| Rust formatting | `cargo make fmt-rust-check` |
| TOML formatting | `cargo make fmt-toml-check` |
| Rust lint | `cargo make lint-rust` |
| Vstyle lint | `cargo make lint-vstyle-rust` |
| Site type check | `cargo make check-node` or `npm --prefix site run check` |
| Site build | `cargo make build` or `npm --prefix site run build` |

`cargo make test` runs `cargo nextest run --workspace --all-targets --all-features`, the
vNext architecture test, and the XY-1308 CLI diagnostic process matrix (`Makefile.toml`).
The CLI matrix builds the real `decodex` binary, binds the real runtime to an isolated
OS-selected loopback port, and proves status/doctor, stable identity mismatch,
disconnection, malformed/missing profile configuration, unsafe server-host paths,
database unavailability, plugin/vault/blob unknown states, and redaction. Protocol unit
fixtures separately force wrong major/minor, malformed/oversized response, timeout, and
untrusted server-text cases.

The XY-1267/XY-1307 integration command and XY-1264 storage proof are intentionally separate because they require an intended macOS host with one PostgreSQL 18 distribution. Each creates and removes its own isolated temporary checksummed cluster with TCP disabled and never enumerates or changes an existing service. The PostgreSQL command provisions fixture-only migration/runtime roles, proves least-privilege daemon bootstrap, and rejects 27 unsafe roots covering direct, inherited, NOINHERIT/SET-only, and membership-admin paths to forbidden role attributes, PostgreSQL 18 namespace-object ownership (including distinct collation, conversion, operator, and text-search cases), DDL, table/ledger/sequence mutation, grant options, `session_replication_role` SET/ALTER SYSTEM, retention bypass, trigger drift, extension-member control, an indirect public-function trigger, and a genuinely additional function. It closes every runtime-callable Decodex function over exact signatures, overloads, metadata, settings, and canonical source. All 34 shipped functions have the exact secure `pg_catalog, decodex` function-local search path. The three shipped security definers are the bounded cursor issuer, bounded cursor/history-version pruner, and trigger-only history-version capture function; runtime cannot insert cursors or execute capture directly. The additional thirty-fifth function is migration-owned, runtime-executable, `SECURITY DEFINER`, configured with an unsafe setting, and is invoked as runtime to perform owner-authority trigger DDL before fixture restoration and independent rejection. The separate substitution fixture replaces a shipped safety body without changing its signature. The indirect-trigger fixture proves runtime DML executes a public definer function despite direct `EXECUTE`, protected-table `UPDATE`, and `TRIGGER` all being denied. The extension fixture proves a public runtime-owned extension can transactionally drop it. Six incompatible roots cover missing ledger SELECT, canonical-function drift, a dropped credential constraint with demonstrated credential insertion, an external child cascade with demonstrated runtime-mediated deletion, a same-count tampered migration ledger, and absent `pgcrypto`. The canonical PostgreSQL 18 schema manifest closes defaults, constraints on both foreign-key sides, indexes, enums, and internal constraint-trigger semantics. Descriptor-pinned socket unit fixtures reject a same-UID pre-planted endpoint in a world-writable configured directory, a mismatched operator UID pin, replaced ancestors, replaced endpoints, and deterministic replacement between precheck and failed connect; an unchanged secure stale socket maps to unreachable. An isolated daemon fixture starts Ready, replaces the configured endpoint, and proves a fresh V1.2 doctor query becomes unsafe-host-path without migration or repinning. The runtime protocol tests keep mutation receipt lookup/capacity independent across V1.1/V1.2 and prove repeated, ordered, concurrent live queries neither replay nor consume receipts. The adapter contract tampers a ledger name at constant row count and removes `pgcrypto` after bootstrap, proving read-only live revalidation reports both as incompatible before restoration. The harness also exercises an in-flight Rust BlobSession across an immediate PostgreSQL restart: the old session loses its hash lock and transaction-B connection, its stale claim cannot complete, and a reassigned exact retry verifies already-published bytes before committing metadata. It also proves `setval` denial, same-signature callable hostile-`search_path` safety, Turkish ICU credential behavior, and populated dump/restore. The XY-1264 proof additionally exercises rollback, blob, and cache behavior (`crates/decodex-postgres/src/socket.rs`, `crates/decodex-runtime/tests/bootstrap_doctor.rs`, `scripts/vnext/postgres_store_test.py`, `spikes/vnext-storage/proof.py`, `spikes/vnext-storage/README.md`).

The PostgreSQL integration harness bootstraps the shipped four-migration history (`V1`
foundation, `V2` claim indexes, forward-only `V3` Conversation history, and forward-only `V4`
account readiness with the honest `unavailable` observation), verifies
transaction/idempotency/revision behavior, Conversation-lock serialization with append-only history-derived
positions, snapshot high-water, and immutable item-version sequence with no writable stored next-position counter, page-only opaque
issued-cursor pagination with never-issued/expired/cross-Conversation/edited-chain rejection,
fixed chain page size, 512-per-Conversation/4,096-global durable limits, serialized concurrent
capacity, exact-boundary expired-chain pruning, runtime direct-root denial, and the canonical
receipt-before-statement-level-hierarchy/cursor/row lock order under same- and cross-Conversation
history-versus-Artifact races, mutation-stable snapshot replay, canonical
insert-time lifecycle timestamps, immutable RuntimeSession Codex-thread and
last-known-turn correlations across lifecycle transitions, scoped foreign-key and terminal-state
counterexamples, contiguous Artifact revision history and exact parent/current-revision coherence,
receipt-first fenced claims, exact stored-response replay, and cross-operation/entity conflict before
effects, large history and Context Pack blob offload, canonical media-type rejection before authority commit,
missing/tampered/retried direct and transitive blob behavior, sorted session hash/per-shard admission
locks, concurrent shard-capacity enforcement, bounded grace-aged resumable orphan reclamation with
metadata-commit-before-byte-removal crash ordering, two-connection parent/child serialization races,
writer/reclaimer exclusion, complete Context Pack provenance/readback/determinism/truncation, sealed
source-manifest append/update/delete/gap rejection, a real PostgreSQL-backed WebSocket history path,
and hard-disabled transition dispatch. A hostile-search fixture supplies same-signature callable
shadows and proves canonical media validation and trigger timestamps still use catalog semantics.
It also verifies collation-independent credential rejection in
a Turkish ICU database, dumps the populated primary database, restores it into a fresh
database, and reruns the restored contract. The primary contract also exercises
caller-shifted lease/retry/retention anchors, early and due delivered-row deletion, and
forbidden outbox truncation. Intermediate schemas from unshipped branches are not
compatibility targets.

## Validation scope selection

Use the aggregate gate before broad readiness, landing, or release-readiness claims. During iteration, choose the smallest command that covers the touched contract, then name that scope in handoff notes. A narrow validation result is useful evidence, but it is not equivalent to the broad gate unless the change is truly limited to that surface.

Good targeted scopes are contract-shaped rather than file-shaped: CLI parsing/output, runtime state transitions, tracker comments, GitHub status and merge behavior, app-server payloads, site type/build behavior, or plugin/generated-artifact sync. If a change crosses scheduler, review/landing, state, or public/private projection boundaries, start with the relevant focused tests and finish with a broader Rust or repo gate when feasible.

## Owner path source map

Use the owner path to choose the first validation surface:

- `crates/decodex-core/`: vNext domain/application contracts and authority ports plus
  the typed `~/.decodex` root, bounded/redacted config, stable server identity,
  integrity-verifying blobs, and disposable bounded cache.
- `crates/decodex-protocol/`: version and loopback server boundary plus the bounded typed
  client transport shared by CLI and future UI clients.
- `crates/decodex-postgres/`: explicit PostgreSQL product-state adapter and isolated real-PostgreSQL integration tests; XY-1307 runtime composition supplies only typed explicit configuration and retains unavailable on every bootstrap failure.
- `crates/decodex-codex/`: typed shared-home Codex adapter foundation; live dispatch is
  default-disabled by the failed XY-1304 gate.
- `crates/decodex-runtime/`: `decodexd` lifecycle assembly over the four narrow owners.
- `apps/decodexd/`, `apps/decodex-cli/`, and `apps/decodex-gpui/`: active vNext composition roots.
- `apps/decodex/`: frozen v0.2 source excluded from the workspace; provenance only.
- `apps/radar/`: Radar auxiliary tool for upstream evidence, release deltas, signal rendering, artifact validation, and ledger workflows.
- `apps/decodex-publisher/`: Publisher auxiliary tool for social candidate, reservation, post validation, and publication handoff workflows.
- `plugins/decodex/`: installable Decodex runtime/operator plugin source, including planning, runtime ops, commit, and landing skills/hooks.
- `automations/radar/` and `automations/decodex/`: repo-local Codex App automation sources; generated Radar and Publisher artifacts stay under `.agent/automations/**/cache`.
- `site/`: Astro/TypeScript public static site and app download entry; validate with site type/build commands rather than runtime checks.
- `apps/decodex-app/`: native SwiftPM macOS app for local account-pool management and bundled Decodex helper/server workflows.
- `spikes/vnext-storage/`: isolated XY-1264 PostgreSQL, blob, and bounded-cache feasibility proof; validate it with `cargo make test-vnext-storage-proof` and use [the evidence record](../evidence/vnext-storage-feasibility.md) for accepted choices and boundaries.
- `scripts/`: repository helpers; `scripts/assets/` owns checked-in asset generation and `scripts/macos/` owns macOS app packaging checks.
- `.github/`: repository automation such as CodeQL code scanning ruleset support.

## Targeted Rust checks

Common targeted commands:

```sh
cargo check --all-features --all-targets --workspace
cargo nextest run --workspace --all-targets --all-features
cargo make test-vnext-architecture
cargo test -p decodex-core --all-targets --all-features
cargo test -p decodex-core -p decodex-protocol -p decodex-postgres -p decodex-codex -p decodex-runtime
cargo test -p radar <filter>
cargo test -p decodex-publisher <filter>
```

The remaining test map on this page describes frozen v0.2 provenance and remains useful
only when later removal work audits preserved behavior. It is not an active vNext test
surface.

## Test map

Use source placement over stale historical test counts; current high-value areas are:

- `apps/decodex/src/orchestrator/tests/`: intake, retry, review/landing, runtime cleanup, operator status, repo gates, Program dispatch, reconciliation.
- `apps/decodex/src/agent/tracker_tool_bridge/tests/`: dynamic tracker tools, continuation guards, review handoff, review repair, closeout, terminal finalize, public/private projections.
- `apps/decodex/src/agent/app_server/tests/` and `apps/decodex/src/agent/json_rpc/tests/`: app-server JSON-RPC parsing, dynamic tools, phase goals, thread/turn runtime, transport failures.
- `apps/decodex/src/state/tests/`: SQLite persistence, leases, run-control channels, protocol replay, schema migrations, runtime records.
- `apps/decodex/src/cli/tests/`: CLI parsing and command contract checks.
- `apps/decodex/src/mcp/tests/`: MCP resources, HTTP transport, CORS/auth, capability profiles, lane/project control tools.
- `apps/decodex/src/config/tests/` and `apps/decodex/src/workflow/tests/`: config and workflow policy parsing.
- `apps/decodex/src/manual/tests/`, `apps/decodex/src/github/tests/`, `apps/decodex/src/worktree/tests/`, `apps/decodex/src/recovery/tests/`: Git, PR, landing, worktree, and recovery helpers.
- `tests/scripts/test_sync_installable_plugins.py`: Python test for installable plugin sync and repo-local global skill cleanup.

When adding tests, protect an externally visible contract: CLI output, status JSON, tracker comments, runtime DB state, app-server protocol payloads, Git commands, or public/private boundary behavior. Behavior families that deserve focused tests include scheduler/intake/retry transitions, review and landing classification, tracker mutation writebacks, app-server JSON-RPC payloads, SQLite state and leases, MCP resources/tools, config/workflow parsing, recovery/retained-worktree flows, and Radar/Publisher artifact validation. Prefer table-driven cases when inputs vary only by spelling or equivalent invalid values; keep separate tests when the state-machine outcome, persisted lifecycle marker, authority boundary, process boundary, or observable public surface differs.

Non-Rust validation matters when the touched surface is not in the Cargo workspace: use `npm --prefix site run check` or `npm --prefix site run build` for the Astro site, the plugin and automation Python commands for generated installable artifacts, and the Swift/macOS staging commands for `apps/decodex-app/`. Do not treat `cargo test` as coverage for those surfaces.

## CLI and operator command discovery

Runtime command surface starts in `apps/decodex/src/cli.rs`. For live command details, prefer:

```sh
decodex --help
decodex <subcommand> --help
```

Important source modules:

- `apps/decodex/src/cli/control_commands/run.rs`: `decodex run`.
- `apps/decodex/src/cli/control_commands/serve.rs`: `decodex serve`.
- `apps/decodex/src/cli/control_commands/status.rs`: `decodex status`.
- `apps/decodex/src/cli/control_commands/lane.rs`: lane inspect/steer/interrupt.
- `apps/decodex/src/cli/control_commands/project.rs`: project registry.
- `apps/decodex/src/cli/control_commands/mcp.rs`: MCP gateway.
- `apps/decodex/src/cli/research_intake_commands/intake.rs`: Program Intake.
- `apps/decodex/src/cli/recovery_commands.rs`: recovery command families.
- `apps/decodex/src/cli/manual_commands.rs`: commit and land.
- `apps/decodex/src/cli/account_commands.rs`: account pool.
- `apps/decodex/src/cli/verify_commands.rs`: validation status publishing.

## Local validation status gate

Frozen v0.2 projects choose `[github].landing_mode` in their `project.toml`. The current
`decodex.example.toml` is the vNext global path/config template and no longer models this
frozen project setting.
The default `standard` mode waits for GitHub's status rollup and ordinary merge
gates. `fast` mode trusts the Decodex local full-check status
`decodex/local-full-check`, requires `landing_actors`, and allows those actors to
execute ruleset bypass landing after local validation passes. The publish command
attaches the local validation status to the exact PR head and base evidence
(`apps/decodex/src/cli/verify_commands.rs`):

```sh
cargo make check
HEAD_SHA="$(git rev-parse HEAD)"
BASE_REF=main
git fetch origin "$BASE_REF"
BASE_SHA="$(git rev-parse "origin/$BASE_REF")"
decodex verify publish-status \
  --config /path/to/project.toml \
  --pr https://github.com/OWNER/REPO/pull/NUMBER \
  --context decodex/local-full-check \
  --state success \
  --expected-head "$HEAD_SHA" \
  --expected-base-ref "$BASE_REF" \
  --expected-base-oid "$BASE_SHA" \
  --description "cargo make check passed"
```

Success requires head/base preconditions, preventing stale green statuses after PR or target branch movement. Publish only after the cited command has passed on the exact tree, and include a description that lets a later operator connect the GitHub status back to the local evidence packet. In fast landing mode, the local status is a merge authority boundary, so a moved PR head, moved base branch, wrong context, or unapproved status creator should stop landing rather than be worked around manually.

## Code scanning

GitHub rulesets for this repository require CodeQL code scanning before merge.
The checked-in workflow is `.github/workflows/codeql.yml` and runs on pushes to
`main`, pull requests targeting `main`, and a weekly schedule. It
analyzes Rust and JavaScript/TypeScript with no-build CodeQL mode so the
required code-scanning tool is configured for PR heads without adding a second
repository build gate.

## App-server compatibility checks

For app-server integration work:

```sh
codex app-server generate-json-schema --experimental --out target/decodex-app-server-schema-check
cargo test -p decodex-codex --all-targets --all-features
cargo test -p decodex-runtime live_read_only_probe_negotiates_without_dispatch -- --ignored
```

Runtime's private supervisor validates the accepted receipt, captures and protects the exact
executable snapshot, then structurally validates canonical generated-schema digests before
app-server spawn. The Codex adapter owns the typed schema/capability contracts but exposes no
launch surface.
Markers are not capability promises. Focused tests cover the golden, exact-build cache
conflicts, scripted fake server, structural history/collaboration-schema rejection, fixed
production command construction, bounded executable/preflight/schema/frame/queue/result
inputs, timeout and descendant/orphan cleanup, typed or hashed untrusted event strings,
shared-home/account re-attestation, redacted debug surfaces, and default-disabled dispatch.
The ignored live test is strictly read-only: `initialize`, `initialized`, `account/read`,
bounded `thread/list(useStateDbOnly=true)`, optional exact-ID
`thread/read(includeTurns=false)`, and fixed-nonmatching-term bounded `thread/search`.
The optional probes prove method availability only and do not establish global title
discovery. Do not replace the live test with excluded v0.2 `decodex probe stdio://`, which
starts a proof turn.

## Plugin and automation checks

Installable plugin sync:

```sh
python3 scripts/config/sync_installable_plugins.py
python3 scripts/config/sync_installable_plugins.py --apply --clean-repo-local-skills
python3 -m unittest tests/scripts/test_sync_installable_plugins.py
```

The Decodex plugin manifest declares runtime package include/exclude patterns.
`scripts/config/sync_installable_plugins.py` must honor that contract when
installing to `$CODEX_HOME/plugins/cache/hack-ink/decodex/<version>`, so
source-only plugin tests are not copied into installed packages.

Codex App automation sync and evaluation:

```sh
python3 automations/decodex/scripts/config/sync_automations.py
python3 automations/decodex/scripts/config/sync_automations.py --apply
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/decodex/automations.toml
python3 automations/decodex/scripts/config/evaluate_automations.py --manifest automations/radar/automations.toml
```

Automation source should stay portable: `{repo_root}` placeholders and relative paths in manifests, with machine-local absolute paths generated only under `$CODEX_HOME/automations` (`automations/decodex/README.md`, `automations/radar/README.md`).

## Static site checks

The site is an Astro/TypeScript static surface (`site/package.json`). Commands:

```sh
npm --prefix site install
npm --prefix site run check
npm --prefix site run build
npm --prefix site run dev
```

Use `site/README.md`, `site/src/`, `site/package.json`, and `openwiki/integrations/plugins-automations-and-auxiliary-tools.md` for the current static-site boundary and validation commands.

## Native macOS app checks

The app is outside the Cargo workspace (`Cargo.toml`). Commands from `apps/decodex-app/README.md`:

```sh
swift build --package-path apps/decodex-app -c release
apps/decodex-app/script/build_and_run.sh
scripts/macos/test_decodex_app_stage.sh
```

The staging script builds Swift and Rust release artifacts, copies `decodex` and `decodex-app-helper` into the app bundle, signs, and verifies the staged layout.

## Radar and Publisher checks

Radar:

```sh
radar --help
radar validate .agent/automations/radar/cache/site-content/signals
cargo test -p radar
```

Publisher:

```sh
decodex-publisher validate-social .agent/automations/decodex/cache/social/x
cargo test -p decodex-publisher
```

Generated Radar artifacts belong under `.agent/automations/radar/cache`; generated Publisher social artifacts belong under `.agent/automations/decodex/cache/social` (`automations/radar/README.md`, `automations/decodex/README.md`).

## Practical change checklist

- CLI option or parsing change: add/adjust `apps/decodex/src/cli/tests/**` and run `cargo test -p decodex cli::tests` or relevant filter.
- Runtime scheduler/lifecycle change: run targeted orchestrator tests, then `cargo nextest run -p decodex` if feasible.
- State schema change: add migration/schema tests and run state tests.
- Public projection change: test redaction and public/private split.
- Plugin hook or installer change: run Python plugin sync tests.
- Site/App change: run the site or Swift checks, not only Rust checks.
