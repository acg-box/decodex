# Keep model catalog requests bound to their source

A normal snapshot refresh incremented the same generation used by model catalog
requests. The completed catalog callback then returned without clearing its task
slot. Later refreshes could not request another catalog. A second regression
showed that a retained catalog remained visible after its native runtime source
changed.

Restore the dedicated catalog request generation and include the snapshot's
runtime source in the catalog context. Snapshot refreshes do not cancel valid
catalog completions. Profile changes and disconnects invalidate requests, cached
catalogs and creation defaults together. An obsolete reply cannot clear the task
slot for a newer request. Retain the current cold-start defaults projection,
draft persistence, custom effort values and Flex behavior.

Both restored regressions fail before the fix. After the fix, all 12 capability
and effort-catalog tests pass, including a new disconnect/new-request case.
The full desktop suite passes after integration with main: 517 tests pass and
5 opt-in tests remain ignored. Strict GPUI lint passes with all features and
targets. This is core request and
source ownership. It adds no optional catalog control or native policy. The two
shared source-file dispositions remain open for their other inherited changes.
Automations stay paused. Signed desktop acceptance remains separate.
