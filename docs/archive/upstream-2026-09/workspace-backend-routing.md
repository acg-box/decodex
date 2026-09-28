> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Current workspace-routing classification

Native Responses routing is core compatibility. Automatic quota activation is an
optional consumer; if retained, its exact-account routing and residency checks
are required. The direct activation path now reads policy through an attested,
ephemerally authenticated native process. It keeps a tool-free, non-persistent
Responses request and the existing no-replay receipt. Account/profile/reset APIs
retain their separate backend origin.

The implementation gap described below is resolved in current source. Live quota
activation and signed desktop acceptance remain separate. See
[current reconciliation](native-policy-routing-reconciliation.md).

## Preserved historical record

The original note follows unchanged. Old binary probes and unresolved-work wording
are historical evidence, not current implementation or acceptance claims.

# Workspace backend routing

## Native authority

Upstream `a4354e2d27fd9a1822ba72617fe2f30d0ef6954b` adds experimental
`account/read.workspaceRouting`. Discovery selects the exact authenticated
workspace, not the backend's default account. The result includes the resolved
HTTPS origin and `NO_CONSTRAINT`, `us`, or `us_cr`. A failed or malformed
discovery is an error, not an unrestricted result. Logout clears the cache;
owner changes discard pending results. Account notifications request a fresh
read and do not authorize later operations.

At cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`, discovery is shared by
routing key, handles native unauthorized recovery, and uses retained session
configuration for model requests. A workspace-bound session rejects a changed
bootstrap origin. Independent custom providers retain their own route.
The final `workspace_routing.rs` owner was read in full. Inventory 1360
(`58e2e8cf`) was reviewed through model-provider, Core request setup, configuration
adapters, redirect handling and routing/authentication tests. Related commits
1461 (`0a5b9991`) and 1549 (`3bb0a530`) still need their complete review.

## Decodex scope, corrected after final caller inspection

The installed alpha.16 schema exposes the field. The foundation account decoder
accepts additive response fields and propagates RPC errors, but does not retain
workspace routing. Native conversation requests use the native provider owner.
Responses delivery still needs qualification through the attested Decodex launch.
Account routing discovery through that launch now has the evidence below.

At the fixed cutoff, model-provider/src/provider.rs::responses_api_provider is
the only caller of apply_workspace_routing. It applies discovery to Responses
HTTP, compaction and WebSocket handshakes. In contrast,
backend-client/src/analytics_session.rs constructs Client::new_without_redirects
from config.chatgpt_base_url and binds account/user identity. The backend client
keeps this base URL for account discovery and reports; it does not consume
workspace_backend_origin. Therefore the earlier requirement to route all usage,
profile, reset-credit and Analytics requests through model workspace discovery
was too broad and is withdrawn. Do not block Analytics on that unproven rule or
send account API credentials to a model-only origin.

A confirmed separate gap is account_api/activation.rs: Decodex sends its own
Responses request to the fixed origin. Resolve this path through native model
routing or an exact-account native routing result before claiming workspace
support for automatic activation. Preserve the selected credential revision,
required-origin conflict checks, redirect rejection and uncertain-effect no-replay
behavior. Do not borrow another task's process. The current cookie adapter does
not implement workspace discovery.

Upstream `bee042d119a4b11068b26386cf8c1e7258bbab58` also moves managed
residency enforcement into `ModelProviderInfo::to_api_provider`. This owner
overrides static and environment-supplied residency headers with the managed
value, including realtime WebSocket and WebRTC connections. Unrelated headers
remain intact. The enforcement remains in that owner at the fixed cutoff.
Native model discovery uses the same provider construction.

The direct activation request bypasses this owner too. Replacing its fixed URL
with `account/read.workspaceRouting.backendOrigin` alone is insufficient. The
installed alpha.16 schema exposes `configRequirements/read.enforceResidency`
inside the requirements object, but a workspace routing result is not a complete
provider request policy. The activation integration must preserve both managed
requirements and workspace routing. This remains an implementation gap.

The installed public `turn/start` schema has no `allowedTools` field. Do not
replace the no-tools activation request with an ordinary agent turn and assume
that an instruction to avoid tools enforces the same behavior. Before selecting
that route, establish a native mechanism that preserves the no-tools boundary,
the non-persistent request and the existing uncertain-effect receipt.

Routing acceptance must cover selected versus default workspace, malformed/failed
discovery, required origins, refresh, logout, concurrent reads and restart. Use
synthetic loopback credentials for activation effects. Account report acceptance
must separately test its backend origin and identity binding; no real reset is
needed.

The installed-native probe `/tmp/decodex-1292-native.py` passes malformed-result
rejection, recovery on a later read, exact selected workspace despite a different
default, cached reads, fresh discovery after restart, and null routing after
logout. Evidence: `/tmp/decodex-1292-native-fixed.log`. The first run failed only
because the fixture rejected unrelated plugin startup GET requests; the corrected
fixture returns 404 for those known paths. This probe uses isolated stdio native
processes, not the Decodex account API or attested production launch path. The
remaining acceptance above is open.

## Attested account discovery acceptance

The installed alpha.16 test
`installed_native_account_nudge_uses_attested_control_and_ephemeral_auth` now
reads native `account/read` through the production attested process before
transferring its connection to the retained bridge. The synthetic discovery
response places a different default workspace first, with a different origin and
routing override. The test verifies the exact selected workspace, origin and
`NO_CONSTRAINT` override. Its subsequent account request still reaches the
configured account backend, and no `auth.json` remains in the isolated home.

PASS: `/tmp/decodex-1360-attested-routing.log`, one test in 11.26 seconds.
Strict runtime lint also passes: `/tmp/decodex-1360-runtime-lint.log`.
This proves account discovery and account API separation through the attested
launch. It does not prove routed Responses delivery, activation, HTTP redirect
handling, credential refresh races or the signed desktop interaction. The new
raw account reader exists only in test support; the production bridge permissions
and activation implementation have not changed.
