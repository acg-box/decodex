# Native temporary structured requests

This is the native request lifecycle for the optional task recap feature. It is
not a complete recap UI or automatic recap producer. The manual catch-up remains
incomplete and its automation remains paused after delivery.

Reference endpoint: 595cc91e8cbb1c2ca822d0311dcf12709410c582.
Source: codex-rs/tui/src/temporary_structured_request.rs and its tests; the recap
consumer is codex-rs/tui/src/app/recap.rs. Installed binary qualification uses
Codex 0.155.0-alpha.16.4 and its generated experimental JSON schema.

## Ownership

The existing AppServerClient sends native config/read, thread/start, turn/start,
turn/interrupt and thread/unsubscribe requests. The retained runtime bridge admits
unsubscribe so a completed private request can detach. No second provider client
or process owner is added.

The caller supplies the selected task's model, provider, native directory and
active permission profile. Native effective configuration and observed MCP names
are combined before disabling every listed server. The temporary thread uses
empty workspace roots, environments, dynamic tools and selected capability roots.
Built-in tools, apps, plugins, hooks, skills and other upstream feature paths are
disabled with native per-thread overrides. Custom permission profiles must be
preserved in readback; other profiles must return read-only sandbox permissions.
The thread must be reported ephemeral before inference can start.

The runtime remains the sole native event receiver. A caller must route the exact
temporary thread's events to this request before inference. The request collector
also checks the thread and turn IDs, keeps the latest completed assistant message,
rejects output above 8 KiB, and requires a successful turn completion. Temporary
server requests fail the operation; this code never grants approvals.

Cancellation before inference only detaches the new thread. Cancellation during
turn/start waits for its exact turn ID, then interrupts that turn and detaches.
Failure and timeout also attempt exact interruption when the ID is available.
The caller must signal cancellation and await cleanup instead of aborting the
request owner. A closed cancellation channel counts as cancellation.

Each native phase has a bounded deadline. No native request is retried. An unknown
start response or broken connection can prevent confirmation of cleanup; the
result remains an error. thread/unsubscribe removes the connection subscription;
upstream explicitly keeps an active turn running after unsubscribe. Therefore
unsubscribe is not treated as proof of interruption or immediate memory eviction.
The shared app-server process is never killed to clean up this optional request.

## Validation and remaining work

Tests cover effective/observed MCP exclusion, permission mismatch cleanup, exact
result identity, failed or incomplete results, response bounds, pre-cancellation,
and cancellation while the turn start reply is pending. Installed-native tests
use synthetic local Responses output and an isolated home, with a required missing
MCP command and a custom permission profile. Both native requests returned the
structured result with no tools in their actual Responses request. The custom
profile was preserved, ephemeral threads were absent from persisted thread/list,
and the source config file remained unchanged. No personal account or production
endpoint is used.

Validation: 824 adapter/runtime tests passed, 57 skipped, across all targets and
features. Strict adapter/runtime Clippy passed. After adding typed unsubscribe
status validation, all four focused tests and the installed-native test passed
again. The initial native fixture omitted default_permissions and was rejected
before inference; the corrected fixture passed. The first runtime Clippy pass
rejected bare unwrap calls in the new test; those now have diagnostic expect
messages. Native interruption timing is covered by the exact-frame cancellation
test; the installed-native test qualifies successful completion and detachment.

The recap feature still needs bounded native-history selection, the recap prompt
and structured result validation, service-owned request identity and cancellation,
source/revision invalidation, and desktop manual/automatic controls. Native request
and signed desktop tests for that complete flow are still required. This transport
batch must not be counted as a completed task recap capability.
