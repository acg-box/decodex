# Live native file approval evidence

Classification: core compatibility for existing file approvals.
Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.

The upstream app-server's `bespoke_event_handling.rs` builds file approval params
from thread, turn, item, reason and grant root. The diff arrives in the preceding
fileChange item notification. Pending native history can omit that item.

## Delivery

Chief retains item evidence under the native connection, thread, turn and item.
It attaches that evidence beside the original request params. It releases the
buffer only after the immutable inbox transaction commits. A duplicate pending
request uses the saved receipt. Completion, thread lifecycle events and connection
closure release stale buffers. The buffer permits 32 items and 32 MiB in total.

Selected request queries reuse the existing file projection to expose saved diffs
through the complete approval reader. Existing native liveness and reply guards
remain authoritative. Native history remains the fallback when no saved item is
available. This does not add automatic decisions or change native permissions.

Schema40 preserves schema39 receipts and its migration ledger while expanding the
local composed envelope bound to 16,842,752 bytes: two native 8 MiB parts plus
64 KiB of routing metadata. Each native part retains its own bound. Protocol2.80
uses the composed bound for complete request text and assembly. Native transport,
ordinary history and unrelated event limits do not change.

## Evidence and limits

Database tests cover a schema39 upgrade, immutable receipts, foreign keys and a
9 MiB composed envelope with individually bounded parts. Runtime tests cover
connection and item isolation, failed database writes, duplicate replay, complete
Unicode page assembly and changed evidence rejection.

Installed Codex `0.155.0-alpha.16.4` produced a pending file change with more than
90 KB of diff in an isolated native fixture. The coordinator saved the complete
diff while native history did not contain its suffix. Explicit decline was sent
once, a repeated reply failed, and the target file was not written. This fixture
uses a loopback provider and is not signed desktop or production account acceptance.

The full database suite passed 129 tests. Runtime passed 579 with 42 opt-in skips;
protocol passed 136 unit and six integration tests. The installed-native fixture
passed separately. Strict all-target, all-feature Clippy passed for core, database,
protocol, runtime and GPUI. Maximum-size end-to-end page latency and signed desktop visual
acceptance remain unqualified. The full manual catch-up is still open.
Automation must remain paused, including after manual completion.
