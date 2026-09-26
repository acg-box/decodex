# Complete Guardian observations

Classification: core compatibility for the existing optional Guardian review UI.
Native review policy and execution remain owned by Codex.

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Upstream commit `9c4879f3a5bfdc7fb5401b7d3ebcdc9a77c27aa2`, which is an
ancestor of that cutoff, preserves complete reviewed actions and uses native
request budgets instead of truncating action arguments.

Decodex previously rejected Guardian observations above 256 KiB in both its
adapter and database. The decoder returned no review, so a valid large native
action could disappear before persistence. Generic large approval request
storage did not cover this separate observation path.

The adapter and database now use the existing 8 MiB native message bound from
`decodex_core::MAX_NATIVE_MESSAGE_BYTES`. Complete event JSON, including action
suffixes, stays attached to the original thread, turn, review and process owner.
No schema migration or execution-policy change is required.

The public review list retains its existing limits: at most eight rows and
128 KiB per page. Details above the 96 KiB display limit remain withheld with an
explanation, and the UI cannot approve a review whose complete details are not
shown. Complete large-action browsing in the desktop is not provided by this
change. Retention does not itself submit approval or wake work.

Validation uses a 300 KB Unicode command with an exact required suffix. The
adapter regression failed at decode before the change. The updated tests cover
complete decode and supported action conversion, database reopen, the native
notification-to-coordinator path, unchanged no-wake/no-auto-approval behavior,
bounded withheld display details and rejection above the native message limit.
These fixtures do not establish live-provider Guardian execution or signed
large-review desktop acceptance.

## Resume configuration ownership

The inherited Guardian approval path rebuilt resume parameters from local task
configuration. Current `chief/native_settings.rs::resume_params` sends only the
thread identity, history exclusion and raw-event subscription. Codex restores
the saved settings. A changed startup default must not replace them.

At fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`app-server/tests/suite/v2/thread_resume.rs` includes
`thread_resume_preserves_acknowledged_model_effort_and_approvals_reviewer_update`.
It verifies saved settings after resume. This is upstream source evidence, not
an execution claim for the installed binary.

The old model-observation call is now owned by the app-server client's guarded
resume response handler. `ServerRequests::observe_permission_hydration` records
permission, plugin and model observations together. `chief_models::inspect`
persists the current observation before it reads the model review. Do not restore
the old, separate observation owner.

A local unloaded-thread regression changes startup model settings and delivers
a native thread-closed notification on the original connection. It verifies one resume without configuration overrides,
one approval for the saved denial, no thread or turn creation, and an unchanged
durable denial event with submitted approval state. This does not establish
live-provider Guardian execution or packaged desktop acceptance.
