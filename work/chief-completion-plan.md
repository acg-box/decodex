# Chief functional completion

## Accepted scope

Complete live conversation, typed requests, recoverable history pagination,
recursive managers, and project/workspace ownership. Preserve the conversation-
first GPUI design and existing command idempotency and process fences.

## Delivery order and acceptance

1. Live transcript and pagination: current-turn partial output appears before
   completion; final output replaces partial output; older pages load without
   duplicates; late responses cannot cross work identities.
2. Typed requests: render question options and text fields, command/file approval
   context and exact offered decisions. No raw JSON required for supported requests.
3. Recursive managers: persist manager ownership; restrict tools and inbox delivery
   to the correct subtree; deliver child results to the nearest manager; recover
   without duplicate dispatch after restart.
4. Workspace/project ownership: persist project directory and work ownership;
   expose selection and creation through the Chief surface without configuration
   forms in the ordinary conversation path.
5. Run service tests and live disposable qualification, stage one native preview,
   and inspect interaction and layout. Update the audit with actual completion.

## Ownership

SQLite owns saved history and work identities. Runtime owns provider events and
execution; protocol owns bounded projections; GPUI owns view state and user input.
Partial output must never become a command inbox trigger or completion evidence.
New protocol shapes require an exact-current version increment. Existing stored
work must remain readable through an additive migration.

## Completion record

All five implementation slices are complete. See [the functional audit](chief-functional-audit.md)
for exact behavior, validation, compatibility handling, and capacity limits.
The implementation keeps control requests behind explicit user decisions and
keeps transport recovery separate from new execution. Native preview staging and
final visual inspection are recorded in the implementation log.
