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
| Rust formatting | `cargo make fmt-rust-check` |
| TOML formatting | `cargo make fmt-toml-check` |
| Rust lint | `cargo make lint-rust` |
| Vstyle lint | `cargo make lint-vstyle-rust` |
| Site type check | `cargo make check-node` or `npm --prefix site run check` |
| Site build | `cargo make build` or `npm --prefix site run build` |

`cargo make test` uses `cargo nextest run --workspace --all-targets --all-features` (`Makefile.toml`).

## Targeted Rust checks

Common targeted commands:

```sh
cargo check --all-features --all-targets --workspace
cargo nextest run --workspace --all-targets --all-features
cargo test -p decodex <filter>
cargo test -p radar <filter>
cargo test -p decodex-publisher <filter>
```

Prefer targeted filters while iterating, then run a broader gate before handoff when the change touches shared runtime behavior.

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

When adding tests, protect an externally visible contract: CLI output, status JSON, tracker comments, runtime DB state, app-server protocol payloads, Git commands, or public/private boundary behavior.

## CLI and operator command discovery

Runtime command surface starts in `apps/decodex/src/cli.rs`. For live command details, prefer:

```sh
decodex --help
decodex <subcommand> --help
```

The compatibility docs readiness command is:

```sh
decodex docs check
```

It checks the current repository documentation surface (`openwiki/` and/or
`docs/`) and keeps downstream docs-readiness automation on a Decodex-owned CLI
contract.

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

Projects choose `[github].landing_mode` in `project.toml` (`decodex.example.toml`).
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

Success requires head/base preconditions, preventing stale green statuses after PR or target branch movement.

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
decodex probe stdio://
```

A passing probe should include `PROBE_OK` (`openwiki/specs/contracts-and-data.md`). Also run relevant Rust tests under `apps/decodex/src/agent/app_server/tests/` and MCP/tracker bridge tests when dynamic tool behavior changes.

## Plugin and automation checks

Installable plugin sync:

```sh
python3 scripts/config/sync_installable_plugins.py
python3 scripts/config/sync_installable_plugins.py --apply --clean-repo-local-skills
python3 -m unittest tests/scripts/test_sync_installable_plugins.py
```

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
