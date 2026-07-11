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

## Validation scope selection

Use the aggregate gate before broad readiness, landing, or release-readiness claims. During iteration, choose the smallest command that covers the touched contract, then name that scope in handoff notes. A narrow validation result is useful evidence, but it is not equivalent to the broad gate unless the change is truly limited to that surface.

Good targeted scopes are contract-shaped rather than file-shaped: CLI parsing/output, runtime state transitions, tracker comments, GitHub status and merge behavior, app-server payloads, site type/build behavior, or plugin/generated-artifact sync. If a change crosses scheduler, review/landing, state, or public/private projection boundaries, start with the relevant focused tests and finish with a broader Rust or repo gate when feasible.

## Owner path source map

Use the owner path to choose the first validation surface:

- `apps/decodex/`: Decodex Rust CLI, runtime, orchestration, tracker/app-server integration, MCP, state, recovery, and operator control-plane behavior.
- `apps/radar/`: Radar auxiliary tool for upstream evidence, release deltas, signal rendering, artifact validation, and ledger workflows.
- `apps/decodex-publisher/`: Publisher auxiliary tool for social candidate, reservation, post validation, and publication handoff workflows.
- `plugins/decodex/`: installable Decodex runtime/operator plugin source, including planning, runtime ops, commit, and landing skills/hooks.
- `automations/radar/` and `automations/decodex/`: repo-local Codex App automation sources; generated Radar and Publisher artifacts stay under `.agent/automations/**/cache`.
- `site/`: Astro/TypeScript public static site and app download entry; validate with site type/build commands rather than runtime checks.
- `apps/decodex-app/`: native SwiftPM macOS app for local account-pool management and bundled Decodex helper/server workflows.
- `scripts/`: repository helpers; `scripts/assets/` owns checked-in asset generation and `scripts/macos/` owns macOS app packaging checks.
- `.github/`: repository automation for language checks, Pages deployment,
  release packaging, and dependency updates.

## Targeted Rust checks

Common targeted commands:

```sh
cargo check --all-features --all-targets --workspace
cargo nextest run --workspace --all-targets --all-features
cargo test -p decodex <filter>
cargo test -p radar <filter>
cargo test -p decodex-publisher <filter>
```

Prefer targeted filters while iterating, then run a broader gate before handoff when the change touches shared runtime behavior. Choose filters from the behavior family or module path that owns the observable contract: `review_landing` for merge/review state, `tracker_tool_bridge` for Linear writes and continuation guards, `app_server` or `json_rpc` for protocol payloads, `state` for leases/migrations/runtime rows, `mcp` for operator control tools, `manual`/`github`/`worktree`/`recovery` for Git and landing helpers, and CLI parser names for command output. If one change crosses families, run one focused filter per family rather than a single file-shaped smoke test.

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

Success requires head/base preconditions, preventing stale green statuses after PR or target branch movement. Publish only after the cited command has passed on the exact tree, and include a description that lets a later operator connect the GitHub status back to the local evidence packet. In fast landing mode, the local status is a merge authority boundary, so a moved PR head, moved base branch, wrong context, or unapproved status creator should stop landing rather than be worked around manually.

## GitHub Actions

The checked-in GitHub Actions workflows under `.github/workflows/` cover:

- `language.yml`: Rust formatting, style, check, clippy, tests, site build/check,
  and TOML formatting for pushes and pull requests targeting `main`, plus merge
  queue groups.
- `deploy-pages.yml`: static Astro site build and GitHub Pages deployment from
  every push to `main`, plus manual dispatch.
- `release.yml`: tagged release packaging for the Rust CLI, macOS app, and
  release assets. Its macOS job stages `target/decodex-app/Decodex.app` and
  verifies the `Decodex` bundle name and `space.decodex.app` identifier before
  packaging.

Dependabot configuration lives in `.github/dependabot.yml` for Cargo, GitHub
Actions, and site npm dependency updates.

Radar upstream review, release checkpoint curation, and artifact retention are
Codex App automations sourced from `automations/radar/`, not GitHub Actions.

## App-server compatibility checks

For app-server integration work:

```sh
codex app-server generate-json-schema --experimental --out target/decodex-app-server-schema-check
decodex probe stdio://
```

Compatibility is capability-gated on the live app-server contract, not on a Codex version string: schema generation must retain Decodex's required method/event/dynamic-tool markers, `decodex probe stdio://` must finish with `PROBE_OK`, and runtime preflight must leave no blocked checks for config, model selection, provider capabilities, skills, plugins, or MCP state. When reviewing probe evidence, pay attention to missing schema markers, final probe output, `command/exec` health output (`COMMAND_EXEC_OK` when that check is enabled), and preflight check statuses/details such as configured/default model, provider capability flags, enabled skill/plugin counts, marketplace load errors, MCP login blockers, or the explicit MCP timeout degradation path (`apps/decodex/src/agent/app_server/preflight.rs`, `apps/decodex/src/agent/app_server/preflight/checks/`, `apps/decodex/src/agent/app_server/schema_probe/constants.rs`). Also run relevant Rust tests under `apps/decodex/src/agent/app_server/tests/` and MCP/tracker bridge tests when dynamic tool behavior changes; changes to dynamic tool dispatch or phase-goal lifecycle need matching app-server tests such as `dynamic_tools` or `phase_goal_tests` coverage, not just a passing probe.

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
