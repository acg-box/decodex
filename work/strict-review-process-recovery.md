# Strict review process ownership recovery

The strict-review notice path accepted a matching running thread and turn without
checking the native process generation. An event from an unbound or retired
process could therefore add a notice to the current task. The restored runtime
regression reproduces the unwanted saved receipt before the fix.

Pass the coordinator's native generation to the existing database method. Check
`chief_process::owns_work` inside the same transaction that finds the running
turn and inserts the resolved receipt. This existing owner requires the current
ready process for the work tree. A direct coordinator without durable process
admission retains its existing behavior; a missing generation cannot bypass a
saved admission. No new ownership abstraction or schema is needed.

The notice remains a historical observation. It does not approve an action, stop
execution, wake work or report a review result. Repeated notices in one turn keep
the first observation. Native Codex still owns the review policy and execution.
This is compatibility repair for the existing notice consumer, not a new review
feature.

## Source and verification

At fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`app-server-protocol/src/protocol/v2/notification.rs` defines thread ID, turn ID
and start time. `app-server/tests/suite/v2/guardian_v2.rs` requires the strict
notice to match the native review-start event. The installed Codex
0.158.0-alpha.2 schema includes `autoApprovalReview/strictReviewRequired`.
These protocol fields do not replace local process ownership.

Restore the preserved database ownership regression exactly. It covers unready,
missing and foreign generations, current owners, generation rotation, duplicate
notices, restart and no pending work or wake. Restore the runtime unbound-process
regression and retain the existing turn-bound and historical-display tests.
These are local notification/storage checks, not live Guardian execution or
signed desktop acceptance.
