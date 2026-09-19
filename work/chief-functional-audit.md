# Chief functional audit

## Delivered scope

The five gaps from the initial audit now have service, protocol, persistence, and
desktop implementations. The personal Chief remains the main conversation entry.

1. **Live conversation.** Persist current-turn assistant deltas in a bounded
   projection. Read saved events and partial output in one database transaction.
   The desktop refreshes every 500 ms and replaces partial output with saved final
   output. Partial output cannot trigger manager dispatch or prove completion.
2. **Typed requests.** Render question options, editable answers, concealed secret
   answers, and explicit command, file, and permission decisions. Responses retain
   the exact work and live request identity. Supported requests require no JSON
   editing. Permission and policy details can display structured JSON for review.
3. **History.** Use immutable event cursors to load earlier pages. Merge by event
   identity, retain readable history on transient failure, and preserve the reading
   position when earlier content arrives. Keep drafts with their manager recipient.
4. **Recursive management.** Persist executable managers separately from passive
   goal groups. Each manager has its own provider thread and coordination tools.
   Tools and inbox delivery stop at the next manager boundary. Child results go to
   their owning manager. Recovery includes durable but not yet bound child work.
5. **Workspaces.** Persist a workspace name and existing project directory with its
   manager. Descendants inherit that directory. Users can ask the Chief to create
   workspaces or child managers, then open them from the sidebar or scoped graph.

## Existing data and provider tools

Protocol 2.17 and additive database migrations through schema 17 carry the new
projections and ownership. Old provider threads cannot receive new dynamic tools
through resume. On the next idle dispatch, the runtime upgrades an old manager by
creating a replacement provider thread, recording the old and new identities, and
keeping the Decodex work identity and saved history. It supplies bounded prior
context as quoted data and sends only the new request. An ambiguous provider
outcome remains uncertain; recovery does not blindly repeat creation or input.

## Verification

- Database: 39 tests passed. Protocol: 74 tests passed.
- Runtime: 296 tests passed; one opt-in test skipped.
- GPUI: 284 tests passed across main and visual targets; six skipped.
- Architecture: 16 tests passed. Strict Clippy, formatting, and diff checks passed.
- Two isolated live hierarchy qualifications passed with gpt-6-astra: personal
  Chief → workspace manager → team manager → worker, four independent threads,
  scoped result disposition, partial output before completion, and timer-driven
  provider reconnect. Recovery issued zero send commands and left zero pending
  host errors. These runs did not modify the user's production conversation.
- Automated tests cover migration, exact turn identity, pagination, manager scope,
  directory inheritance, draft ownership, and legacy tool upgrade without replay.
- Connection failures now close after an attested same-account reconnect. Repeated
  failed probes retain one pending error; a later failure creates a new event.
  This receipt does not resolve the task or dispatch a turn. Native service
  readback confirmed the old pending connection error cleared automatically and
  the existing Chief and provider thread identities stayed unchanged.

## Explicit limits

The live view is a 500 ms polling projection. Saved pages and live text have encoded
size limits; source readback can already be truncated. Pagination does not restore
text that the provider readback did not save. Graph snapshots retain the existing
100-work-record bound. Workspace managers inherit the personal Chief's account,
model, and execution policy. Old-manager context transfer is bounded to 100 saved
events and 256 KiB; visible saved history remains pageable. Live qualification
covered provider reconnect, while restart and migration paths have automated
coverage. These checks do not prove every possible model-generated organization.

## Real supervision acceptance

A user-authorized read-only review ran in the normal Decodex workspace through the
existing Chief, one project manager, and two workers. Both workers read source and
reported findings. The manager reviewed their results, then the root accepted the
project report. The UI now shows durable reporting edges and recent real activity.
The previous interactive simulation and its playback controls have been removed.

The review prompted repairs to manager redispatch acceptance, durable follow-up
instructions, bounded final-answer retention, draft preservation, and activity
availability before the first conversation result. Targeted database/runtime/GPUI
tests pass. The real review records remain available for the user to inspect.
