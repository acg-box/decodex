---
type: "Evidence"
title: "Built-in Domain Pack Pressure Test V1 Evidence"
description: "Implementation, capability, GPUI, live execution, and restart evidence for Development and Paper Investment built-in Domain Packs."
tags: [adaptive-factory, domain-pack, ontology, graph, sqlite, gpui, paper-investment, evidence]
openwiki:
  roles: [testing, architecture, workflow]
  change_kinds: [lifecycle, public-api, validation]
  source_paths: [database/migrations/0007_builtin_domain_pack_binding.sql, database/src/program_cycles.rs, crates/decodex-protocol/src/domain_pack.rs, crates/decodex-protocol/src/program_cycle.rs, crates/decodex-runtime/src/domain_packs.rs, crates/decodex-runtime/src/application.rs, crates/decodex-runtime/domain_packs/decodex.dev-1.0.0.json, crates/decodex-runtime/domain_packs/decodex.paper-investment-1.0.0.json, crates/decodex-runtime/fixtures/us_treasury_yield_curve_2025_06.csv, apps/decodex-gpui/src/programs.rs, apps/decodex-gpui/src/factory_surface.rs]
  test_paths: [database/src/program_cycles.rs, crates/decodex-protocol/src/domain_pack.rs, crates/decodex-runtime/src/domain_packs.rs, crates/decodex-runtime/src/application.rs, apps/decodex-gpui/src/programs.rs, apps/decodex-gpui/src/factory_surface.rs, apps/decodex-gpui/src/client_lifecycle/tests.rs]
  invariants: [SQLite stores one immutable Program Pack identity only.; Pack versions and manifest digests are exact.; Domain types and relations are bounded and namespaced.; Domain entity identities are stable derivations.; Capabilities are denied unless the exact Pack grants them.; Program Pack admission precedes QuickTaskRuntime and ProviderAttempt creation.; GPUI owns all Pack rendering.; The paper fixture is frozen and runtime-offline.; The Paper Investment Pack has no order or external-action capability.]
  validation_commands: [python3 scripts/vnext/local_database_gate.py, python3 -m unittest tests/scripts/test_vnext_architecture.py, cargo test -p decodex-protocol -p decodex-database -p decodex-runtime, DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer cargo test -p decodex-gpui --features visual-capture, DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer cargo test -p decodex-gpui --bin decodex-gpui --features visual-capture client_lifecycle::tests::live_daemon_completes_the_builtin_domain_pack_pressure_test -- --ignored --exact --nocapture]
---

# Built-in Domain Pack Pressure Test V1 Evidence

Status: implemented, locally verified, and live dogfooded.

Date: 2026-08-16.

This page contains no credential, account identity, provider thread identity, provider
Turn identity, or conversation content.

## Question and result

This milestone tested one architecture question:

> Can two materially different domains use one Program, Quick Task, ProviderAttempt,
> evidence, protocol, and GPUI kernel while adding only declarative vocabulary and a
> derived domain view?

The result is yes for the two bounded fixtures. Software development and paper
investment research use one kernel without adding their nouns to its lifecycle model.
No second scheduler, worker engine, generic entity store, graph database, ontology
runtime, or executable plugin host was required.

The result does not prove a public extension system. Both Packs remain compiled in.

## Delivered boundary

SQLite schema 7 adds `program_domain_pack_bindings`. A new Program stores its exact Pack
ID, version, digest, and binding time in the same transaction as the first Program cycle.
One existing legacy Program can receive one revision-fenced binding. Update and delete
triggers make the binding immutable.

Protocol V2.4 adds bounded Pack descriptors, capability declarations, entity and
relation declarations, and one domain projection on `ProgramCycleDto`. It also adds the
legacy binding command. A descriptor requires an exact semantic version, SHA-256 digest,
bounded namespace, unique declared capabilities, and unique namespaced entity and
relation types.

`decodexd` embeds exactly two JSON manifests:

| Pack | Version | Manifest digest | Namespace | Granted capability |
| --- | --- | --- | --- | --- |
| `decodex.dev` | `1.0.0` | `cdecdff922ef1ec29fbe48cc5b72877fa70cce564bbb783272dd47ce614dc146` | `dev` | `codex.quick_task` |
| `decodex.paper-investment` | `1.0.0` | `996a5133a30bc968d27a16835bdbdb34736777c9d11ca2a5ed87d221c957e9eb` | `finance` | `codex.quick_task` |

The Development projection derives Repository, Change, and Validation entities from
current Program records. The Paper Investment projection derives two Asset entities,
one Thesis, and one Scenario. Entity IDs use one domain-separated SHA-256 derivation over
the Program ID, exact Pack digest, and local entity key. The projection is not persisted.

## Frozen paper fixture

The Paper Investment Pack embeds 20 observations from the official U.S. Treasury daily
par yield curve for June 2025. Runtime use has no network or credential dependency.

Source:
`https://home.treasury.gov/resource-center/data-chart-center/interest-rates/pages/xml?data=daily_treasury_yield_curve&field_tdr_date_value_month=202506`

Fixture SHA-256:
`1736087dfc077c238d8ab206629c4ccf9a2cb127e21b0cd91a53e5d0d4b0daf7`

The deterministic 10-year minus 2-year results are:

- first spread: 52 basis points;
- last spread: 52 basis points;
- minimum: 44 basis points;
- maximum: 56 basis points;
- range: 12 basis points; and
- every observation has a positive spread.

The Pack declares no market-data, order, broker, paper-order, or real-money capability.

## Fail-before-attempt boundary

A Program Quick Task performs Pack admission before it selects `QuickTaskCapability` and
before it calls `QuickTaskRuntime::create`. The exact WorkItem owner is read from SQLite.
The built-in registry then checks the immutable ID, version, digest, and requested
capability.

One SQLite-backed runtime test creates Programs with a missing binding, unknown Pack,
and mismatched digest. It also requests an undeclared capability from a valid Pack. Each
case returns a closed error while the ProviderAttempt page remains empty. Protocol tests
reject invalid namespaces and declarations before a projection exists. Ordinary Quick
Tasks without a Program WorkItem keep their prior path.

## GPUI proof

The Program intake has one explicit Pack selector. The selection becomes immutable when
the Program is created. A bounded Treasury example fills the paper research fields but
requires the user to select an absolute working directory. A legacy Program can bind one
Pack through an exact revision-fenced control.

The selected Program surface renders these host-owned views from one readback:

- Program pulse and Pack identity;
- domain graph cards and named relations;
- causal Program graph and timeline;
- Pack version, digest, namespace, schema counts, and capability state;
- selected domain entity fields and source;
- evidence and review state; and
- the existing Codex conversation path.

GPUI visual-capture tests pass, and the 1490 by 1092 paper view was inspected. The Pack
does not inject visual code or receive a GPUI handle.

## Automated evidence

The local database gate reports schema 7, seven exact migration digests, 40 exact tables,
WAL, integrity checks, and the immutable Pack binding. The architecture suite passes 10
tests.

Focused suites pass with these results:

- protocol: 72 unit tests and 6 local-transport tests;
- database: 20 unit tests and 1 restart integration test;
- runtime: 195 unit tests, 7 bootstrap tests, 5 supervised-validation tests, 20
  WebSocket tests, and 3 compile-fail documentation tests; and
- GPUI with visual capture: 129 main tests and 19 factory-capture tests before the live
  test was added.

The expected installed-Codex, real-daemon, and CLI-wrapper tests remain explicitly
ignored in ordinary automated runs.

## Live two-domain evidence

Before live migration, an SQLite online backup passed `integrity_check`. The live
baseline was schema 6 with one three-cycle Development Program and 45 ProviderAttempts.
The signed V2.4 local service migrated the database to schema 7 and retained all six
accounts. All eight required core doctor checks became Ready. The intentionally disabled
ManagedRepository and optional not-probed capabilities kept their typed prior states.

The existing three-cycle Development dogfood Program received `decodex.dev`. This one
immutable binding advanced its Program revision once. It retained three Signals, three
WorkItems, and three Reviews and created no ProviderAttempt.

The live pressure-test client then created one `June Treasury Curve Research` Program
with `decodex.paper-investment`. Its WorkItem used the normal account route,
RuntimeSession, ProcessGeneration, Codex app-server, and ProviderAttempt owners. The
attempt reached `succeeded`. The Program recorded two Evidence rows and one
`knowledge_progress` Review. The total ProviderAttempt count changed exactly from 45 to
46.

The installer then restarted the daemon. Readback retained the server identity, both
Program revisions, both Pack IDs and digests, the paper Conversation and ProviderAttempt,
and exactly 46 total ProviderAttempts. A second run of the same live pressure test
completed read-only in 0.39 seconds. It asserted all three Development and all four Paper
domain entity IDs and created no Conversation or ProviderAttempt.

## Finding and retained limits

The pressure test supports the smallest architecture:

- one immutable Pack binding in SQLite;
- one compiled-in declarative registry in `decodexd`;
- one bounded protocol projection; and
- one GPUI host renderer.

It rejects a generic domain-entity store for this milestone. Program records remain the
authority, and the domain graph remains a deterministic lens.

This evidence does not validate third-party Packs, Pack installation, public schema
evolution, automatic Program continuation, background stewardship, dynamic multi-agent
execution, MCP actions, live market data, paper orders, real-money actions, or profitable
investment behavior. A public SDK should wait for a concrete external authoring need or
a third real domain that cannot remain built in.
