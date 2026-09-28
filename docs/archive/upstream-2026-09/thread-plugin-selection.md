> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Current task-plugin classification

The task plugin selector is an implemented optional product control. Native Codex
owns plugin activation, tool filtering and enterprise registration. Decodex uses
the current shared observation/journal owners and changes only the reviewed task
exclusion list. Queued and unknown writes are not silently retried.

Current transport publications have their own revisions. Re-reading one revision
keeps its identity; a new publication can invalidate a review even if values match.
The older duplicate-observation wording and protocol/schema versions below must
not be used as the current contract. See [settings observation mapping](native-settings-observation-tests.md)
and [plugin controller qualification](native-plugin-controller-qualification.md).

For the removal decision, distinguish this selector from shared plugin installation
and the native security/runtime rules. Canonical connectors, enterprise policy,
root-only suggestions and Sites loading remain native-owned. Their historical
source review does not prove real remote-service acceptance. See
[the optional controls inventory](optional-controls-reconciliation.md).

## Preserved historical record

The original note follows unchanged. Its versions, probe results, queued-review
status and remaining-work wording describe the preserved branch only.

# Thread plugin selection

Status: implementation and targeted native qualification complete for source
1156. The native adapter, transport observation, service persistence, durable
command and desktop controls are implemented. Signed desktop acceptance remains
part of the full manual catch-up; this note does not claim final delivery.
The service protocol is now 2.62; database schema 41 is unchanged.

Upstream `8570091e14dc794b256402e27a7773e8de132f3e` applies a thread's
`disabledPluginIds` to skills, recommendations, hooks and model-visible MCP/App
tools. Selection changes activate at the next admitted task. Shared installed
plugin configuration stays unchanged. Direct Apps RPC access is separate from
model tool filtering. Plugin identity also separates MCP approval keys and
persistent tool approvals.

The fixed cutoff is `595cc91e8cbb1c2ca822d0311dcf12709410c582`. Its activation
function is unchanged. Later `af1fc2dbff641e78298c272c4b45c9fec5c33898` changes
shared connector selection: disabling a canonical owner can exclude a connector
even if another enabled plugin contributes it. All 500 lines of that later patch
were read while checking the cutoff behavior, and its canonical-owner function
is unchanged at the cutoff. Its inventory position is still queued. Do not
reproduce the earlier shared-owner rule in Decodex.

Installed Codex qualification uses an isolated home, a local Responses server and
a local stdio MCP plugin. `/tmp/decodex-1156-native.py` and its result JSON show:

- An enabled plugin advertises its tool.
- A settings update alone leaves the current tool environment unchanged.
- The next turn removes the disabled plugin's tool; re-enabling restores it.
- A cold resume retains the disabled list; shared config bytes stay unchanged.

This test does not cover hooks, Apps, shared canonical connectors or desktop
interaction. The generated schema still describes selection without capability
filtering; that description does not match the measured binary behavior.

`app_server_client/thread_plugins.rs` now projects bounded native selection facts
and provides one guarded, non-retrying `thread/settings/update` call. Missing
selection is unknown; an explicit empty list clears it. Its acknowledgment means
queued, not confirmed. The retained-process bridge accepts only the exact plugin
selection shape and rejects unrelated permission/model/configuration edits.

The existing bounded permission-observation implementation now shares a generic
settings store with plugin observations. Start/resume replies and settings
notifications update separate typed projections in wire order. A new publication
invalidates old write guards. Active turns suppress writable observations until
completion. Missing or malformed data invalidates previous facts. Revert, close
and connection termination clear observations. This does not add a second event
consumer or a native configuration owner.

The coordinator records source-bound `native_task_plugins` events in the existing
resolved event journal. No migration is required. Duplicate observations share a
revision; A-to-B-to-A retains the transition; unavailable facts remain explicit.
The task history excludes these internal records. Persistence tests cover exact
thread/generation ownership, no model wakeup, private settings omission, and
database reopen. A stored selection remains evidence only; the future command
must also require the live transport observation and guard.

Validation: all Codex adapter tests pass (180 passed, 5 ignored, plus one doc
test); all six task-settings runtime tests pass. See temporary evidence logs
`/tmp/decodex-1156-codex-full.log` and `/tmp/decodex-1156-persistence.log`.
Actual cross-client publication and desktop acceptance remain open.

The command owner now checks the exact source, current native observation, saved
observation revision and reviewed catalog before changing one ID. It preserves
all other exclusions. A resolved journal reservation precedes its only native
write. Unconfirmed selections block dispatch and other setting mutations across
restart. An RPC result is immutable and separate from a current native settings
publication. A replacement owner can settle an old attempt only after confirmed
old-process death. Native state confirmation does not claim that this client
caused the observed change. Shared configuration is not edited.

The desktop panel shows shared installation state separately from task exclusion.
It labels the next-turn behavior and exposes saved unknown plugin IDs for removal
from the exclusion list. Each click reads state again after the command, including
after lost replies. Source, task, turn and profile transitions invalidate the
review. Existing accessible buttons support click, Enter and Space.

Additional evidence: two durable selection database tests cover competing
reservations, restart, unknown/rejected/queued results, stale reviews, publication
before acknowledgment and no replay. The service test preserves unrelated IDs,
rejects changed source metadata and sends only once for queued or lost replies.
Two GPUI tests exercise the public Unix socket, rendered click, lost reply,
pending readback and review invalidation. The complete Chief filter has 169
passing tests and one ignored test. Protocol tests have 127 unit and six
integration passes. See `/tmp/decodex-1156-selection-db.log`,
`/tmp/decodex-1156-owner-final.log`, `/tmp/decodex-1156-picker-final.log`,
`/tmp/decodex-1156-chief-ui.log` and `/tmp/decodex-1156-protocol.log`.
The final rendered socket test also removes an unknown saved exclusion when
shared plugin discovery is unavailable: two tests pass in
`/tmp/decodex-1156-picker-restore-final.log`. Full database tests pass (114 unit
and five restart tests). Strict runtime, GPUI and database Clippy passes in
`/tmp/decodex-1156-final-command-lint.log`; the last change after that lint only
extends the rendered test fixture.

The installed-native controller test now crosses the retained bridge and uses
the real coordinator to process settings notifications. The durable receipt moves
from queued to target observed. Replaying the old review fails. Reopening the
service database retains confirmation; cold native resume retains the excluded
plugin. No extra model turn or shared config write occurs. The isolated fixture
includes a registered project marketplace; a cache directory alone is not an
installed-plugin discovery fixture. See `/tmp/decodex-1156-native-controller-final.log`.

Two independent WebSocket clients observe identical settings publications, and
one client can remove only one of the other's exclusions. A new native process
retains the resulting selection. See `/tmp/decodex-1156-cross-client-result.json`.
This tests native multi-client delivery, not two full Decodex desktop processes.

The database recovery regression covers unknown old-process death, refusal of
premature ownership transfer, confirmed death, new-generation observations and
database reopen. Old-owner or incomplete facts cannot settle a reservation.
Current target facts settle it; a different current selection marks it superseded.
See `/tmp/decodex-1156-owner-recovery-final.log` (one test, three cases).

Remaining full-goal acceptance: signed desktop interaction and combined service
process lifecycle across features. Native hooks and Apps canonical filtering
were source-reviewed, not exercised against real external services. No local
plugin/tool filter was added. Later unrelated MCP UI, metadata and elicitation
changes in the cutoff delta remain assigned to their own queued commits.


Source 1196 (`c62d191c4c8c0cab7045fca6efc399197334bb6c`) adds the
app-server settings contract consumed above. Full patch and final thread-manager
delta were read. Omission and null preserve selection; an empty list clears it.
The cutoff restores fork selection from retained history, not the parent's current
selection. Decodex does not send `thread/fork` or replace plugin IDs on each turn.
Its explicit settings command and start/resume/notification projections apply.

Installed-native qualification in `/tmp/decodex-1196-native-changed.log` covers
replacement, omission and null alongside changed model settings, without inference.
After two turns, clearing the list and restarting, current forks retain the empty
selection while `lastTurnId` and `beforeTurnId` restore the earlier selection.
Only two local model requests occur; shared configuration is unchanged. This
qualifies cold paginated forks, not all legacy and warm fork combinations or
full Decodex desktop forking. An unchanged settings request need not emit a new
notification. A fresh thread without rollout history cannot be resumed yet.

## Enterprise MCP registrations

Source 1262 (`374c4b2d828321cfa3d3fc69ce66870f8b7fd01f`) binds enterprise
authentication when the native catalog resolves its winning registrations.
Activation requires trusted `use_xaa` and identity-provider configuration.
Installed and selected plugins use the same endpoint and resource policy. A
rejected registration does not permanently disable a replacement hosted app with
the same server name. Catalog rebuilds retain the enterprise policy and bind new
materialized settings again. Decodex must not replace this with ordinary OAuth
or rewrite plugin endpoints. The catalog and registration owners are unchanged
at the fixed upstream cutoff.

The installed-native regression runs in two independent processes and rejects
seven project overrides: OAuth and ChatGPT auth, endpoint, resource, scopes,
client ID and authorization-server issuer. It also rejects interactive OAuth
fallback and confirms that the trusted configuration stays unchanged. Evidence:
`/tmp/decodex-1262-native.log`. These checks use temporary configuration and no
real credentials. They do not qualify enterprise login, plugin installation,
selected-plugin endpoint rejection or catalog replacement during approval.


## Canonical shared-connector ownership

Reviewed upstream `af1fc2dbff641e78298c272c4b45c9fec5c33898` and the final
connector snapshot owner. Disabling a canonical plugin excludes its connector
even if another enabled plugin contributes it or its bundle is absent locally.
Canonical ownership is matched by plugin name and marketplace, and remote cache
metadata is used only for the current authentication owner. Disabling an ordinary
contributor still permits an enabled shared owner to expose the connector.

Decodex preserves exact plugin IDs, existing exclusions and the native settings
confirmation path. It does not compute connector visibility from local plugin
cards. No protocol change is required. The earlier native plugin tests do not
qualify this canonical remote-owner case; an installed-native fixture for shared
contributors, canonical disable/clear and account-cache changes remains open.

## Root-only installation suggestions (1356)

Reviewed `7f83d4922d7e92a36c1c1e4f61159a5815d45360` and its final handler delta.
Native request_plugin_install rejects non-root sessions before parsing either
legacy connector or recommended-plugin arguments and emits no installation
elicitation. Final elicitation failures return a model-visible error. Decodex's
installation owner requires a live exact native request guard and reviewed catalog
target; it does not recreate request_plugin_install. Keep native root ownership.
Unit/integration tests for both upstream formats were read, not run. Installed
subagent rejection through the full Decodex path remains unqualified; this source
review does not establish it from ordinary root installation tests.

## Native Sites compatibility guard (1440)

Upstream `108e6a6d` removes account/backend exclusion files, migration throttling
and catalog-time installed-snapshot waits. When the remote catalog is active, the
runtime loader gives its loadable cached remote Sites entry precedence over the
bundled identity, even if the remote entry is disabled. A missing remote bundle
retains the bundled fallback. This rule applies to effective loading, not a new
client-side rule for hiding catalog entries or rejecting installation.

Decodex keeps the native `plugin/installed` result for the owned task cwd, with
full IDs, explicit enabled state and marketplace errors. There is no local Sites
migration state to remove. Do not deduplicate these identities by display name or
copy the runtime guard into the UI. Upstream three-case agent-turn tests cover
enabled, disabled and missing remote bundles; installed remote-service/cache
qualification remains distinct. No change to the retired website is involved.
