---
type: Reference
title: "Local database operations"
description: "Current-only SQLite initialization, account routing and durable credential and branch records."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-5eddd1d872eed61f8ba7e703
    resource: repo://apps/decodex-gpui/src/agent_draft_storage.rs
  - id: openwiki-source-f339a4d1910db63ca305c2df
    resource: repo://apps/decodex-gpui/src/assistant_settings.rs
  - id: openwiki-source-546812ff3637690f4a184b89
    resource: repo://apps/decodex-gpui/src/ui_preferences.rs
  - id: openwiki-source-69e9fd7efd8c3cd616184e27
    resource: repo://crates/decodex-components/src/ragflow_service.rs
  - id: openwiki-source-9ecfe8c96199e765ca2b4e1b
    resource: repo://crates/decodex-components/src/ragflow.rs
  - id: openwiki-source-caeefdea0805519b59a469aa
    resource: repo://crates/decodex-protocol/src/desktop_drafts.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-eac747e105c1c8d9e7e3c811
    resource: repo://crates/decodex-runtime/examples/clear_development_data.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-2da6601c3f30e806c504e991
    resource: repo://crates/decodex-runtime/src/host_credentials.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-003f00974f49a9f106bbcbea
    resource: repo://database/schema.sql
  - id: openwiki-source-58c0167d31c3e40dc89591ab
    resource: repo://database/src/agent_fork.rs
  - id: openwiki-source-fc5ad70343bb20c4e9d12540
    resource: repo://database/src/agent_process/attachments.rs
  - id: openwiki-source-d31590aa1b62ee057094fb38
    resource: repo://database/src/agent_process/tests/attachments.rs
  - id: openwiki-source-82bb214209944301d2efacfb
    resource: repo://database/src/agent_titles.rs
  - id: openwiki-source-6197a876b2effbe992b1e2ec
    resource: repo://database/src/agent.rs
  - id: openwiki-source-86596187dc9df4e756031ef3
    resource: repo://database/src/agent/deletion.rs
  - id: openwiki-source-3f60f3a3aae45b3e3b355d3e
    resource: repo://database/src/assistant.rs
  - id: openwiki-source-7c3b932da49e58b50b150e50
    resource: repo://database/src/development_reset.rs
  - id: openwiki-source-d1279ad243eb550afec4d0e6
    resource: repo://database/src/guardian_settings.rs
  - id: openwiki-source-dc8958560b1230967470835f
    resource: repo://database/src/schema.rs
  - id: openwiki-source-28af4781ad8d9558410630da
    resource: repo://database/src/works.rs
  - id: openwiki-source-c0eb738b42368debe7ef7545
    resource: repo://database/src/works/commands.rs
  - id: openwiki-source-960cb6b925f1fa45c737a735
    resource: repo://scripts/macos/verify_decodex_bundle_contracts.py
generated: { by: "codex", at: "2026-11-06T17:37:20.544Z" }
verified:
  - by: openwiki/0.9.0
    at: 2026-11-06T17:37:20.544Z
---

# Local database operations

## Current schema and ownership

`database/schema.sql` defines the current product schema, version 61. `database/src/schema.rs` initializes empty databases directly and verifies existing databases against that exact application ID, version and structure. There is no migration ledger or historical account-transfer tool. An older database is refused without resetting its contents.

Project membership, persistent Agent responsibilities, Work, requirements, Materials, Results, Collaboration Requests, Guardian settings and execution receipts are current records in that schema. Native history and current editable input retain separate owners. Program and Domain Pack storage is retired.

Work commands validate the current owner, expected requirements, holds and related evidence inside an immediate transaction. A committed command ID replays only the exact same actor and content. Completion requires current related Results and handled required requests. Old Results stay historical unless a new reuse assessment relates them to current requirements. SQL applies Agent and granted Material scope before bounded reads.

Native contribution intent and exact direct-call identities are attribution records, not another executor or mailbox. The current schema includes `native_work_deliveries`: one immutable receipt per contribution with exact parent thread/turn, child thread/receiving turn and message identity. A foreign key requires the contribution, and native child/turn/message identity is unique. An identical store-method retry is idempotent; changed identities conflict. A missing received-input receipt remains missing; the service does not invent historical input evidence.

`agent_process_bindings` permits separate immutable Agent bindings on one admitted native process. Each new attachment requires the exact registered Agent/native thread and a ready admitted generation, and rejects a competing live generation. Account and native process authority checks remain in place.

`assignment_proof` distinguishes `plaintext` from `native_task_name`. A binding recovered from native task metadata and parent activity can exist without a received-input receipt. That absence stays explicit and does not become plaintext proof.

The runtime can save actual native input before parent activity arrives, then join the receipt with exact native history after restart or during `work_read`. It does not replay an instruction or spawn. Missing or ambiguous binding remains Unknown. Peer requests use the existing inbox and exact delivered-turn receipts; restart does not imply acceptance.

Ordinary input resumes its original native context and recorded tool declarations. The runtime does not maintain a tool-context upgrade journal or replace a thread because the local tool revision changed. Unconfirmed external effects require authoritative readback.

Global Guardian stores the Review Details opt-in and private review records. Conversation permission settings have a separate owner. Private records are deduplicated by reviewer thread, turn and item. Records are bounded to 64 KiB and the newest 128; small snapshot previews and exact selected reads stay separate. These records do not grant execution authority or complete Work.

Local clients and service must agree on exact protocol 4.8. Publication-instance and command receipts preserve reconnect and idempotent recovery. Exact native bundle ABI checks remain a separate verification boundary. See [commands](commands-and-validation.md) and [the canonical contract](../decisions/project-agent-work.md).

## Authority and paths

The sole normal product store is `~/.decodex/server/decodex.sqlite3`, owned by `decodex serve`. GPUI, the menu-bar library and short-lived CLI commands use the local protocol. They must not inspect or mutate SQLite directly as a fallback.

The store uses a serialized connection, owner-private paths and bundled SQLite. The verifier checks the current schema, foreign keys and integrity. This validation is not permission to rebuild a retained database.

## Installation and upgrade

Select the app-bundled CLI installation or the standalone local-service installation. Do not install two competing service owners. The app includes a signed helper and native-client/menu-bar libraries; service and UI compatibility is checked. Clients must match `CURRENT_VERSION` from the local protocol crate. An incompatible service is a version problem, not proof that history has disappeared.

Use `decodex --help` and `decodex serve --help` from the installed artifact before operating a host. Use the repository stage/install scripts for that installation mode. Qualify retained data conversion as an explicit operator task before switching to a different schema. A documentation refresh does not authorize deletion of retained data.

The product has no automatic old-format converter. For a retained installation, stop its owning service, verify a one-time conversion against the target schema, and preserve native identity, history, current input and unresolved effects. A fresh database test alone does not prove retained-data preservation.

## Account routing and recovery

Current Route is a synchronous service-owned operation. The service locks routing and the target account, reads current revisions, refreshes when required, checks shared-auth source identity and external-client liveness, performs exact projection/readback, and commits the authoritative result. A running auth-owning Codex client or source drift can reject the operation. The old Pending Route receipt, 100 ms recovery loop, and process-blocker DTO are no longer the current contract.

Same-account refresh remains serialized through the credential owner. A valid non-older shared-auth winner can be adopted instead of writing back a losing token. Never print token values while diagnosing this path.

Agent conversation ownership is a different boundary: unavailable conversations reject new input and preserve readable history. Checking availability is not a request to resend messages. See [Agent coordination](../architecture/chief-coordination.md).

## Validation without touching production data

```sh
python3 scripts/vnext/local_database_gate.py
cargo +stable test -p decodex-database --lib
cargo +stable test -p decodex-runtime --lib
scripts/macos/test_decodex_app_stage.sh
```

The database gate and unit tests use isolated fixtures. The staging test checks the signed bundle's native compatibility. These do not authorize migration experiments on the user's root.

## Related operations

- [Reset Cards](reset-cards.md): explicit redemption and durable uncertain outcomes.
- [Weekly quota activation](quota-activation.md): deduplicated minimal subscription request.
- [Commands and validation](commands-and-validation.md): active tools and evidence boundaries.

## Current format boundary

The embedded schema defines one current layout. Conversation requests store one required `service_tier`; they do not store a second `fast` flag. The service stores quota activation preferences. The App owns menu-bar visibility and automatic recap preferences in its local preference store. Desktop drafts use format 12 with explicit creation intent and one service tier. The application has no ordered upgrade path and does not import obsolete account vaults. Current OAuth, PAT, native process and receipt formats remain supported according to their distinct owners.

## Credential and branch records

OAuth credentials use their current version-1 payload and fingerprint representation. PAT credentials use version 2 and retain verified user identity without fabricated OAuth refresh or expiry values. These are distinct supported credential kinds. The current schema accepts both and enforces their dependent authority directly; no old credential migration runs at startup.

Explicit conversation branches reuse durable inbox event records for their intent, native acknowledgement and expected visible history. Ordinary branches expose their reviewed source prefix. Attached children expose only their own turns, so creation is acknowledged against an empty child view while the saved source IDs retain the reviewed boundary. Native inference keeps the parent context. Each target has its own work and native thread identity. Side chats retain the exact source Agent as parent and keep its Project, role and tool revision. The read-only list selects the newest 100 saved attached receipts for that exact source; it adds no schema migration. Before-input branches reuse the existing canonical prompt-edit draft and upload records. Unknown creation does not authorize a second request. See [conversation recovery](../workflows/conversations-and-recovery.md).

Native conversation topics use resolved `conversation_title` observations in the existing indexed inbox journal. The observation keeps the native name, automatic-name provenance and the last successful review checkpoint. It does not wake work or consume transcript pages. Work snapshots join these topics to current native bindings in the same transaction. Agent identity and business Work titles remain separate; the database schema stays at version 61. See [automatic titles](../workflows/conversations-and-recovery.md#automatic-conversation-titles).

## Assistant records

The built-in Assistant is available by default. Its first use creates an idle Agent and selects its role in the existing record transaction. A repeated setup identity reuses that Agent. It does not add a user event or native binding. RAGFlow API keys remain in the private component store; Assistant settings contain a credential reference.

The managed RAGFlow component keeps its private installation receipt, generated service account and origin-bound API credential under the per-user component store. The receipt records the owned Compose project, endpoint, actual version, image, Memory setup and model configuration. Each model stores its own Base URL, model ID and private API key. The receipt uses only the current configuration format. The service directory uses mode 0700 and private files use mode 0600. RAGFlow owns its separate service database, Infinity data, object storage and queue volumes. No model runtime or model data volume is installed. Update reuses the model settings and service data; Uninstall stops the owned project and clears the Assistant connection while retaining volumes and private identity for reinstall. These operations do not change the Decodex SQLite schema.

Assistant configuration, team membership, independent wakes, context intent/acknowledgment/bootstrap receipts and external import receipts reuse the immutable `agent_inbox_events` journal. They use the `assistant_internal_*` event namespace and stay out of ordinary transcript pages. Public `assistant_message` and delegated `assistant_instruction` retain their separate meanings.

A context transition changes the existing Agent binding and appends the existing `agent_thread_revisions` relationship in one immediate transaction. It preserves queued input, Work and independent duties. Exact native creation and import receipts remain authoritative after a missing reply; semantic Memory is not the delivery ledger. Explicit native Agent archive cancels future duties, while retirement of an old Assistant context does not call archive cleanup.

The current schema is 61. No migration chain, duplicate history store or RAGFlow database tables are added. See [Assistant lifecycle](../decisions/assistant-lifecycle.md).

## Delete one conversation

Conversation deletion and archive have different data semantics. Archive retains history. After all recorded native contexts of a deleted conversation are absent, `agent/deletion.rs` removes its local inbox content, partial outputs, usage, requests, context revisions and other conversation state in one transaction. The owner enables SQLite `secure_delete` and checkpoints the WAL. The desktop clears the conversation's saved drafts and cached history.

The database keeps content-free native deletion receipts so a repeated command remains idempotent. Independently owned Work, Results and collaboration references remain valid. When such a record still requires an Agent identity or instruction receipt, only an anonymous identity or empty receipt remains; the deleted conversation title, instructions and message body are removed.

The selected Assistant retains its stable identity and saved Memory connection. Deleting its conversation clears the native binding and restores the saved settings with a fresh export boundary. Learned external Memory remains available and old chat content is not reimported. A partial native deletion retains the pending plan and stays visible in Archive for retry.

## Clear a development workspace

The explicit `clear_development_data` maintenance example owns the offline cutover. It uses native Codex APIs to delete exact registered and Decodex-origin contexts, then resets the managed RAGFlow Memory. The database owner clears conversation, Work, collaboration, Assistant duties, history and execution tables in one transaction. Deferred foreign-key checks allow linked records to be removed together. Project lead and member bindings are cleared. Account credentials and operations, account policy, quota-reset receipts, global model defaults and project folder configuration remain.

The operation rejects an active Agent turn. The caller must stop the local service before Apply; the example checks the service socket. After commit, the owner checkpoints and compacts the database and verifies the current schema and foreign keys. The example removes client drafts, blobs, cache, old database backups and logs. It does not retain old chat content or replay earlier inputs. This is a development data reset, not a schema upgrade.

Use [the plan and Apply commands](commands-and-validation.md#explicit-development-reset). The database regression checks that old Agent data disappears while account identity, model configuration and project folders remain, with cleared project responsibility bindings.
