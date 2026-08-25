---
type: "Reference"
title: "Lane Authority V2 Target Contract"
openwiki_generated: true
---

# Lane Authority V2 Target Contract

Status: superseded and frozen by the [vNext authority decision](../decisions/vnext-authority.md).
Retained as historical design and incident provenance only; do not implement this
contract or advance C1-C7.

## Core Records

### ProjectBinding

```text
ProjectKey = UUID
RepositoryKey = (provider, immutable repository database id)
TrackerScopeKey = (provider, workspace id)
routing_predicate = versioned immutable team/label-id expression
binding_revision
revision_state = current | historical
semantic_config_fingerprint
```

`project_key` is generated once, stored in the registered project contract and runtime,
and never derived from an alias, path, or repository text. A repository or tracker
workspace identity change creates a new project. Predicate changes create a new binding
revision under the existing project key. Existing lanes keep their admitted revision and
stop on drift until explicitly rebound or transferred.

One active binding owns one RepositoryKey. Routing evaluates all active bindings and
must produce exactly one match. Zero matches rejects admission. Multiple matches reject
and quarantine the request as `routing_predicate_overlap`. Team movement changes
eligibility, not issue identity. Multi-repository work uses project-scoped child issues.

Exactly one current revision exists per ProjectKey. Routing evaluates current revisions
only. Historical revisions remain immutable for lane attestation and explicit rebind.
Publishing a revision atomically marks the old revision historical and the new revision
current after repository uniqueness and global predicate-overlap validation.

`ProjectAlias(alias, alias_epoch, ProjectKey)`,
`RepositoryLocatorProjection(RepositoryKey, locator_epoch, owner/repository)`, and
`HostCheckoutAttestation(host_id, ProjectKey, git_common_dir_resource_id,
host_config_fingerprint)` are separate mutable projections/resources. Alias rename or
repository rename/transfer locator refresh or checkout relocation cannot change
ProjectBinding fingerprint or invalidate Lane attestation. Repository transfer that
changes immutable repository database id still creates a new project. The semantic
config fingerprint uses canonical path-independent policy,
RepositoryKey, TrackerScopeKey, routing predicate, and content digests; it excludes
aliases, absolute paths, credentials, environment, host ids, and checkout locations.

### RoutingCatalog

```text
routing_catalog_epoch
catalog_digest
current_binding_and_availability_digest
authority_event_id
```

Catalog bytes are deterministic CBOR with domain `decodex.routing-catalog/1`. Entries are
sorted by raw 16-byte ProjectKey and contain exactly ProjectKey, current binding revision/
fingerprint, immutable RepositoryKey/TrackerScopeKey, canonical predicate digest, and
ProjectAvailability state/epoch. Integers use shortest encoding and enums use frozen
numeric tags; SQL row order, aliases, locators, paths, clocks, and map iteration never
enter the bytes. `current_binding_and_availability_digest` hashes the canonical entry
array. `catalog_digest` hashes `domain || predicate_schema_version || entry_digest`;
RoutingCatalog epoch is stored beside but not inside the semantic digest.

Registration publication, binding revision activation, pause, resume, retire, and
project-quarantine adjudication plan against one RoutingCatalog epoch. Finalization
reloads every current binding/availability, re-evaluates repository uniqueness and all
active predicate intersections including the candidate, CASes the expected catalog
epoch, increments it, and commits the binding/availability/current pointer/event in the
same AuthorityTransaction. RoutingIssueSnapshot selection and every admission/effect
selection record the observed catalog epoch/digest as evidence. Two publications planned
against the same catalog cannot both activate: one CAS wins and the loser replans from
fresh catalog.
RepositoryKey uniqueness covers every current non-retired project, including paused;
predicate-overlap checks cover active projects only.
First registration always creates a paused candidate, so publication validates predicate
syntax/canonicalization but permits overlap with active or paused predicates because the
candidate does not route. Multiple paused predicates may overlap. `resume` and migration
of an explicitly active project include the candidate in active overlap evaluation and
reject on any intersection.

RoutingCatalog epoch is publication serialization, not a semantic lane prerequisite.
Each accepted selection creates immutable `RoutingAttestation(attestation_id,
TrackerIssueKey, RoutingIssueSnapshot digest, selected ProjectKey/binding/availability,
catalog epoch/digest, candidate-result digest)`. Lane operations seal the attestation id
and semantic selected-binding facts, not the global epoch alone.

Before forward/retry/reconciliation-write/compensation, `reattest_operation` evaluates a
fresh complete snapshot against the current catalog. If the same ProjectKey,
RepositoryKey, binding revision/fingerprint and availability remain the unique winner,
an AuthorityTransaction CASes the operation's prior attestation id and appends a new
immutable attestation/event without changing the effect plan or desired-state digests.
Unrelated catalog changes therefore do not strand work. Observation-only reconciliation
reads of an unknown effect may run against its immutable target before re-attestation.
If semantic routing changed, readback classifies whether the old effect applied, then a
typed drift-recovery transition can finalize/abort/block the operation; no forward replay
occurs and transfer/rebind waits only until that classified operation leaves active state.

Routing predicate schema v1 is a normalized finite disjunction of clauses. Each clause
contains a non-empty finite set of immutable team ids, a set of required immutable label
ids, and a set of forbidden immutable label ids. Arbitrary expressions and negation are
not accepted. Normalization sorts/deduplicates ids, rejects a required/forbidden
intersection, removes duplicate clauses, and uses canonical length-delimited bytes for
the fingerprint. Two clauses overlap exactly when their team sets intersect and neither
clause requires a label forbidden by the other; two predicates overlap when any clause
pair overlaps. Publication runs this deterministic intersection against every active
binding. Unknown predicate versions fail closed; a schema upgrade creates a new binding
revision and must prove old/new evaluation fixtures.

Routing never evaluates a partial provider object. `RoutingIssueSnapshot` contains the
immutable issue/workspace id, team id, fully paginated immutable label-id set, provider
update/version token, and canonical digest. The adapter exhausts every label page and
requires `has_next_page=false`; a provider cap or `labels_complete=false` rejects.
Without provider snapshot isolation, each pass is
`metadata_start(version, team) -> all label pages -> metadata_end(version, team)` and is
valid only when start=end and the provider capability proves that version changes cover
team/label mutations. It then requires two consecutive valid passes with identical
version, team, and canonical label set. Mutation before, during, or after either page
traversal invalidates a pass; bounded instability fails closed. If current Linear
`updated_at` cannot be proven to cover label changes, label-bearing routing predicates
are unsupported for that workspace and must be replaced or quarantined before C7.
Routing, intake, dispatch, rebind, transfer, and every effect revalidation use this
snapshot or a stronger conditional provider token. No Program, claim, operation, or
effect plan persists from an incomplete or torn snapshot.

Each adapter publishes immutable `ProviderSnapshotCapability(provider, adapter_version,
workspace_id, object_kind, token_kind, token_scope_fields, pagination_scope,
conditional_read_support, proof_fixture_digest, observed_provider_version, expires_at)`.
`token_scope_fields` is an explicit set drawn from `issue_team`, `issue_labels`,
`issue_state`, `issue_relations`, `issue_archive`, and `issue_body`; no token is assumed to
cover an omitted field. The complete snapshot algorithm requires the capability to cover
every routing-predicate field and metadata/page mutation used by the pass. A page cursor
alone is not a version token. Adapter upgrades, provider API-version changes, expiry, or
failed live capability fixtures disable the affected predicate class before resolution.

The current Linear adapter may advertise label-bearing routing only after a sandbox
fixture proves that adding/removing a page-two label changes the same issue version token
used to bracket every metadata/page request; unchanged `updated_at` or missing official
coverage makes `issue_labels` unsupported. Team-only routing is allowed only when the
capability independently proves `issue_team` immutable or version-covered. Otherwise the
workspace is non-routing and existing bindings are paused/quarantined; repeated reads are
not treated as atomic isolation. ADM-08 mutates every covered field before, within, and
after both passes, including a mutation that deliberately leaves an unrelated timestamp
unchanged.

### Binding publication

ProjectKey uses RFC 4122 UUID bytes with canonical lowercase hyphenated text. Normal
registration allocates one CSPRNG UUID once and persists it in a project-scoped
`ProjectPublication` operation before filesystem mutation. A binding/revision moves
through `pending_contract -> contract_published -> current`:

Contract hashing is non-self-referential. Canonical semantic fields excluding
`contract_content_fingerprint` are length-delimited and SHA-256 hashed; that fingerprint
is inserted into the canonical document, then SHA-256 of the final exact file bytes is
the `contract_file_digest`. The file contains only the content fingerprint. The pending
DB row/journal stores both digests and exact bytes; no digest includes itself.

1. AuthorityTransaction writes the pending ProjectKey/revision, both contract digests,
   current-file precondition, and complete filesystem effect; pending rows never route.
2. `filesystem.project_contract.write` atomically publishes/fsyncs the canonical contract
   containing ProjectKey, revision, and digest, then reconciles exact bytes.
3. `finalize_project_publication` reloads/CASes RoutingCatalog, verifies contract
   attestation and repository uniqueness, advances the binding current pointer, and for
   first registration creates `ProjectAvailability(epoch=1, state=paused)` in the same
   AuthorityTransaction before appending the event/completing the operation. No
   registered project can lack availability. A separate `resume` transition performs
   global predicate intersection and activates routing.

Revision publication uses the same protocol and existing ProjectKey. Startup reconciles
every pending publication before routing; disagreement blocks the project and never
chooses DB or file by recency. Crash recovery cannot expose a current binding until both
copies agree.

Migration publishes all proven bindings plus their explicit initial availability states
and one catalog epoch/digest in a single v2 database transaction after global uniqueness/
intersection validation; it cannot expose a partially cataloged project set.

Offline migration has a separate immutable `MigrationPlan` artifact. `plan` allocates
random ProjectKeys once for every proven project root, records no key for project
quarantine, includes source/contract/classifier digests and canonical UUID bytes, and is
fsynced before dry-run. Dry-run and apply require the exact plan digest and therefore use
the same keys and classification; neither regenerates UUIDs. Plan replacement requires
an explicit abandon record before cutover.

### ProjectAvailability

```text
project_key
availability_epoch
state = active | paused | retired
reason_code
authority_event_id
```

Only `active` current bindings participate in routing. `pause`, `resume`, and `retire`
are project-scoped kernel transitions, not direct project-row updates. Pause requires no
invoking effect, increments the availability epoch, fences admission/dispatch and new
forward effects, and leaves existing lanes stopped with only reconciliation, controlled
abort, or cleanup actions. Resume performs repository uniqueness and global predicate
overlap validation before atomically becoming active. Retire requires no nonterminal
Lane, quarantine reservation, operation, claim, resource, or nonterminal ExecutionGroup;
it preserves all binding/history rows. Project deletion and cascading authority-history
deletion are forbidden. Availability transitions append immutable epoch records and
atomically advance one current pointer. `retired` is terminal; a future registration uses
a new ProjectKey rather than resuming or deleting the retired identity.
Every transition also CASes/increments RoutingCatalog in the same transaction; removal
transitions cannot leave a stale catalog digest.

Pause also CAS-rebases every non-invoking active operation from the old availability
epoch to the new epoch, increments its claimant epoch, and sets
`execution_permission=convergence_only` in the same transaction. Stale workers therefore
fail before invocation. Convergence-only operations may perform observation readback,
semantic-same paused re-attestation, compensation, abort finalization, and terminal
cleanup, but no forward publication. Unknown effects first reconcile without replay.
Resume never silently restores forward permission; each operation needs a fresh active
re-attestation/replan. Pause rejects while any effect process/invocation is still live.

### ProjectBindingQuarantine

Ambiguous legacy repository/tracker identity creates a project-scoped quarantine keyed
by migration source project and candidate immutable identities, with a quarantine epoch.
It has no ProjectKey,
binding current pointer, or ProjectAvailability; routing, resume, admission, transfer
destination, and forward effects reject. It does not become a TrackerIssue graph edge.
Typed project adjudication requires independently proven RepositoryKey/TrackerScopeKey,
accountable operator, distinct reviewer, and a parent
`ProjectQuarantineAdjudication` operation. It allocates the explicit resolve-or-split
ProjectKeys, inserts one pending ProjectPublication per result, and plans every canonical
contract-write effect before invocation. The source quarantine remains active throughout
contract publication. After every exact contract attests, one batch
`finalize_project_publications` AuthorityTransaction revalidates all source-node mappings
and global repository/predicate rules, CASes RoutingCatalog/quarantine epoch, activates
all bindings with paused availability epoch 1, maps every dependent source node exactly
once, resolves the quarantine, and finalizes the parent. Crash/replay cannot expose a
partial split or clear quarantine early.
If one of N contract effects fails, abort compensates successful writes in reverse order:
existing preimages restore exactly, absent preimages delete only the unchanged
operation-created file. Any CAS failure leaves `orphan_contract_blocked` and preserves
the source quarantine for recovery; it never activates a partial project set.
Unresolved project quarantine remains non-executable and is visible to authority audit;
identity is never inferred from local path or newest row.

### TrackerIssueKey

```text
TrackerIssueKey = (provider, workspace_id, immutable_issue_id)
mutable projections = team_id, identifier, title, updated_at
```

The immutable provider/workspace/issue tuple is used in uniqueness constraints. Mutable
identifiers and team membership cannot create or transfer authority.

### Tracker workspace and issue resolution

`TrackerWorkspaceDirectory(epoch)` is provider-level bootstrap authority independent of
ProjectKey. It maps immutable `(provider, workspace_id)` to an authenticated account/
credential reference and current provider locator; ProjectBindings reference its
TrackerScopeKey but cannot choose its credential or lookup scope.

The directory is built only from a host-level `TrackerCredentialCatalog`, not project
configuration. Adding a credential reference performs provider token introspection,
reads immutable provider account and workspace/organization ids, and publishes a
directory revision through AuthorityTransaction. Two credentials claiming the same
account/workspace must prove the same immutable ids and capability set; disagreement or
one credential spanning ambiguous workspaces creates a non-routing credential quarantine.
Secrets remain in KeyProtector and the directory stores only opaque credential refs.
Provider URL hosts and mutable workspace slugs are locator projections that resolve to a
directory entry before issue lookup; they never select a project.

Authority-bearing intake accepts only:

- canonical `(provider, workspace_id, immutable_issue_id)`; or
- `(provider, workspace_id, mutable_identifier)`, which must resolve uniquely through
  that workspace directory to the immutable issue id before routing.

Bare identifiers and caller-selected project/config/token lookups are rejected. An issue
URL/slug is first mapped to immutable workspace id by the directory and rejects zero/
multiple workspaces. `IssueResolutionRequest` records selector fingerprint, workspace
directory epoch, InvocationIdentity and provider readback under an unbound
`issue_resolution` subject. Its successful AuthorityTransaction creates TrackerIssueKey
and RoutingRequest; rejection creates only a resolution event. No ProjectKey, binding,
Program, occupancy, or project-scoped credential exists before immutable resolution.

The v2 issue-batch CLI accepts only workspace-qualified immutable keys, qualified
identifiers, or canonical provider URLs. It removes `--config` and `--project-id` as
authority selectors; an optional `--expect-project <alias-or-key>` is a post-routing
assertion and cannot choose credentials, workspace, or candidates. Passing either legacy
selector rejects at parsing with `legacy_project_selector_unsupported`. Accepted Decision
Contract intake remains project-authorized because the accepted contract already carries
ProjectKey and binding fingerprint; any existing-issue reference inside that contract
still goes through the same unbound resolver. C1 removes the project-scoped
`IssueTracker::get_issue_by_identifier` boundary and introduces separate sealed
`IssueResolver` and project projection/effect interfaces, so current config-first client
construction cannot remain reachable in v2.

### RoutingRequest

Before ProjectKey selection, intake persists
`RoutingRequest(RoutingRequestId, TrackerIssueKey, RoutingIssueSnapshot digest,
RoutingCatalog epoch/digest, candidate-result digest, InvocationIdentity)` under an
unbound routing subject. Zero matches atomically appends a rejected decision event and no
occupancy/Program. Multiple matches atomically appends the overlap event and creates
TrackerIssueOccupancy `quarantine` plus TrackerIssueQuarantine carrying candidate
evidence, without fabricating ProjectKey, binding, Lane, IntakeAuthority, or Program.
Exactly one match consumes the routing request in the admission AuthorityTransaction.

### Lane

```text
lane_id = (ProjectKey, TrackerIssueKey)
tracker_issue_identifier
admitted_binding_revision
admitted_binding_fingerprint
epoch
state = admitted | dispatching | executing | review | landing | closeout |
        terminal_cleanup_pending | terminal | quarantined
active_operation_id?
branch?
worktree?
pr?
intake_authority_id
latest_authority_event_id
```

The lane projection is the only current ownership/lifecycle answer. Resource tables may
describe a lease, worktree, attempt, review, or effect, but they do not independently
grant ownership.

### TrackerIssueOccupancy And Claim

`TrackerIssueOccupancy` is the single row keyed by TrackerIssueKey for every active
reservation:

```text
kind = published_intake(IntakeIntentId) | lane_claim(LaneId) |
       quarantine(TrackerIssueQuarantineId)
epoch, authority_event_id
```

AuthorityTransaction changes occupancy kind/owner by epoch CAS; separate claim,
published-intake, and quarantine metadata cannot exist without the matching occupancy.
This gives database-enforced mutual exclusion across pre-admission issue publication,
executable Lane claims, and quarantine rather than cross-table best effort.

A unique non-terminal claim binds one TrackerIssueKey to one LaneId. Historical and
quarantined lanes do not hold it. Transfer releases the predecessor claim and acquires
the successor claim in the same runtime transaction. A non-terminal executable lane
without exactly one claim is invalid and dispatch-blocked.

### ExecutionGroup

```text
execution_group_id, project_key, intake_authority_id
epoch
state = planned | active | draining | terminal | quarantined
terminal_event_id?
```

An ExecutionGroup is the Program-level grouping of one or more LaneIds under one typed
IntakeAuthority and ProjectKey. It is not ownership authority and cannot grant a tracker
claim. It exists so shared Program facts and migration quarantine propagate only across
explicitly mapped lanes, never across every lane that references the same project or
connector root. Membership is append-only
`ExecutionGroupMembership(membership_id, group_id, LaneId, ProgramNodeId,
membership_version, disposition=active|transferred|terminal|quarantined,
supersedes_membership_id?)`. A mapping is never removed or updated; transfer/terminal
disposition appends a version and atomically advances the group's current-membership
pointer. Adding/disposing membership or changing group state is an AuthorityTransaction
event with group epoch plus fresh binding/lane epochs. Terminal prerequisites evaluate
only latest membership dispositions while all historical versions remain queryable.

`planned|active|draining|quarantined` are nonterminal. The kernel may transition to
`terminal` only when every Program node and mapped Lane is terminal, no group/Lane
operation or published occupancy remains, and all required closeout events are committed.
Terminal state is authoritative, not inferred on read. Historical terminal groups and
mappings remain queryable and do not block project retirement; any nonterminal group
does.

### IntakeAuthority

`IntakeAuthority` is an immutable typed union with common authority id, ProjectKey,
current binding/availability/catalog attestation, RoutingIssueSnapshot digest,
InvocationIdentity/correlation, accepted timestamp, and fingerprint:

```text
decision_contract { accepted_contract_id, contract_fingerprint }
issue_batch { accepted_intake_id, batch_fingerprint }
transfer {
  transfer_authority_id,
  source_lane_id,
  source_intake_authority_id,
  source_provenance_fingerprint,
  destination_binding_attestation,
  transfer_causation_event_id
}
```

The transfer variant preserves the original decision-contract/issue-batch provenance by
reference but attests the destination independently; it never copies the source binding
or fabricates an issue batch. A valid issue batch still requires no Decision Contract.

### TrackerIssueQuarantine

A unique quarantine record is keyed by TrackerIssueKey and references every
quarantined lane component and unrecoverable tombstone. `admit`, `dispatch`, and
`transfer` reject while it exists. Only `adjudicate_quarantine` can clear it with one of
these typed outcomes: `release_as_history`, `transfer_to_project`, or `terminalize`.
Adjudication records accountable operator and reviewer identities plus exact evidence.

## Authority Disposition And Invariants

All current authority families have an explicit v2 disposition:

| Current family | V2 disposition |
| --- | --- |
| `projects` registration/enabled/delete state | Migrated to immutable ProjectBinding plus epoch-fenced ProjectAvailability; direct/cascading deletion is removed. |
| ambiguous legacy project registration | Migrated to ProjectBindingQuarantine without a ProjectKey until dual-accountable resolve/split adjudication. |
| `schema_meta` | Replaced by v2 runtime-format, migration-generation, and point-of-no-return metadata; not lane authority. |
| global `leases` | Replaced by lane-owned execution resource rows; table and issue-only APIs removed. |
| issue-claim, dispatch-lock, and internal lock records/files | Replaced by TrackerIssueOccupancy, operation claimant CAS, and supervisor OS-generation lock. Legacy readers/writers/artifacts are inventoried, migrated as evidence/quarantine where needed, then removed; they cannot grant v2 authority. |
| global `worktrees` | Replaced by lane-owned worktree resource rows; table and issue-only APIs removed. |
| `run_attempts` and run control | Reference LaneId; run id remains an event identity, not lane ownership. |
| protocol events, protocol summaries, and run-activity summaries | Remain run-bound diagnostic evidence reached through a LaneId-bound attempt. |
| `review_lifecycle_records` | Current state migrates into Lane plus authority events; legacy table/readers/writers removed. |
| `review_policy_checkpoints` | LaneId-bound review-gate evidence; cannot advance Lane outside `commit_transition`. |
| `evidence_artifacts` | LaneId-bound validation evidence; not current ownership. |
| Program Intake and issue mappings | Reference LaneId and typed IntakeAuthority; they do not grant current ownership. |
| `linear_execution_events` | Public projection receipts referencing LaneId and source authority event; not authority. |
| loop-guardrail checkpoints | LaneId-bound policy evidence; no ownership grant. |
| connector backoffs | ProjectBinding-scoped connector state; not lane authority. |
| Decision Contracts and autonomy objective/signal/proposal rows | ProjectBinding-scoped planning authority; execution still requires typed IntakeAuthority and Lane admission. |
| active/queue/attention labels and Linear comments | Public projections only. |
| private execution events | Diagnostic evidence retained; only lane authority events can advance Lane. |
| inferred conflict occupancy | Replaced by lane-owned conflict leases. |
| activity/control marker files and terminal guard files | Diagnostic/recovery observations only; cannot recreate claims or Lane state. |
| manual-authority closeout receipts | Migrated to a project/Lane-scoped authority event when all immutable identities are proven; otherwise retained only as a typed diagnostic or quarantine tombstone. The legacy reader and writer are removed. |

`LaneStore::commit_transition` is the only writer of current lane authority. It updates
Lane, TrackerIssueClaim, conflict leases, resource disposition, and the authority event
in one SQLite transaction. Required invariants are:

- executable non-terminal Lane implies exactly one `lane_claim` occupancy and matching
  TrackerIssueClaim;
- `active_operation_id` identifies one non-terminal operation with the same lane epoch;
- a conflict lease references one non-terminal owning Lane;
- entering `terminal_cleanup_pending` commits terminal lifecycle authority and releases
  tracker/conflict claims; it is non-executable and may retain only a worktree resource
  plus one fenced cleanup operation;
- cleanup failure stays `terminal_cleanup_pending` with reason-coded retry ownership;
  only successful cleanup advances to `terminal`;
- terminal Lane has no tracker occupancy/claim, conflict lease, active operation, worktree, lease,
  or active control channel; and
- resource/projection disagreement blocks execution and records authority drift.

C1 must prove that every new v2 authority write is dominated by
`LaneStore::commit_transition`, and the machine inventory must enumerate all old schema
readers/writers. During C1-C6, v2 runtime mutation is unreachable while v12 remains the
sole active authority. C7 removes every legacy authority reader/writer from the final
runtime except the read-only offline migration decoder and reverse-scans the complete
source tree. A facade over the old global tables does not satisfy the final contract.

### AuthorityTransaction

One private `AuthorityTransaction` owns the SQLite transaction handle for authority
planning and commit. Stores cannot open nested or independent write transactions. In one
commit it:

- compares Lane epoch, claim, binding revision, and availability epoch;
- appends the authority/selection event;
- inserts the AuthorityOperation and its complete ordered Effect set;
- sets `Lane.active_operation_id` and the operation's claimed lane epoch;
- applies Lane/claim/conflict/resource changes only through
  `LaneStore::commit_transition`; and
- writes causally owned IntakeAuthority, Program, mapping, and execution-group rows.

Every planned operation has at least one effect or a typed `runtime_only` transition
result. Failure at any statement rolls back all of the above, so no active operation can
exist without its complete effects/event, and no Program can exist without its Lane and
selection event.

Goal intake first persists a unique `IntakeIntentId` under `UNIQUE(ProjectKey,
TrustedRequestKey)`. TrustedRequestKey comes from an accepted contract+node, intake id,
or supervisor-issued request nonce, never free-form caller metadata; retries resolve the
existing mapping. The project-scoped parent
operation has one immutable complete effect set containing `linear.issue.create`; its
idempotency key contains the private IntakeIntentId, while its provider text contains
only `PublicIntakeMarker = SHA-256("decodex-intake/1" || IntakeIntentId)` in a typed
machine field. Effects are never added to an existing operation.

Automated create is enabled only when the provider capability probe proves an immutable
server-side idempotency key accepted by create and a lookup/readback by that same key.
The marker is secondary diagnosis, never the idempotency primitive. The current Linear
adapter has no such primitive, so Linear goal-intake issue creation is unsupported and
must reject before `invoking`; operators use a pre-created issue through issue-batch
intake. Unsupported capability cannot be weakened to marker-only retry.

Before a receipt exists, the durable parent operation plus unique provider marker is the
only publication owner; the issue is not dispatch-eligible because no Lane/claim exists.
All-page marker reconciliation must recover its TrackerIssueKey before any continuation
or new create attempt.

When create reconciliation obtains the immutable TrackerIssueKey, the receipt
transaction also inserts a unique `PublishedTrackerIssueReservation(IntakeIntentId,
TrackerIssueKey, parent_operation_id)`, changes TrackerIssueOccupancy from absent to
`published_intake`, and creates exactly one deterministic continuation
operation under `UNIQUE(parent_operation_id, continuation_kind)`. The continuation's
complete effect set is now targetable: it either performs runtime-only admission or
closes/archives the issue. Parent completion is forbidden until the continuation
finalizes. This receipt/reservation/continuation expansion is one AuthorityTransaction;
a failure leaves the parent effect `unknown` or `observed_succeeded`, and replay repeats
the same marker readback and deterministic transaction.

Successful admission atomically converts occupancy from `published_intake` to
`lane_claim`, consumes the published-issue reservation, finalizes the
parent and admission continuation, and creates the Lane, claim, admission event,
IntakeAuthority, Program/mappings, and any lane-scoped follow-up operation. Cleanup
finalization removes `published_intake` occupancy and consumes the reservation only
after close/archive readback. Duplicate
IntakeIntentId or provider issue id cannot create a second reservation, child operation,
Lane, or executable orphan. An operation subject is never rewritten from project to
lane.

If the returned TrackerIssueKey already has occupancy from another intake, receipt
handling creates immutable `PublicationOwnershipCollision`, performs no cleanup or
adoption, and attempts an epoch-CAS transition of that occupancy to quarantine. A live
operation that prevents quarantine is blocked before its next effect and requires typed
adjudication. Provider-idempotent replay never invokes a second create and never treats a
mutable/removed public marker as absence.

### AuthorityOperation And Effect

```text
operation_id
subject = issue_resolution(IssueResolutionRequestId) |
          routing_request(RoutingRequestId) | project(ProjectKey) |
          project_quarantine(ProjectQuarantineId) | lane(LaneId) |
          migration(MigrationId)
intake_intent_id?
parent_operation_id?
continuation_kind?
transition_kind
execution_permission = forward | convergence_only
expected_authority_version =
  issue_resolution(TrackerWorkspaceDirectoryEpoch, selector_fingerprint) |
  routing_request(TrackerIssueKey, occupancy_epoch_or_absent,
                  routing_catalog_epoch) |
  registration_absent(RepositoryKey, TrackerScopeKey, routing_catalog_epoch) |
  project_catalog(ProjectKey, binding_revision, availability_epoch,
                  routing_catalog_epoch) |
  project(ProjectKey, binding_revision, availability_epoch) |
  project_quarantine(ProjectQuarantineId, quarantine_epoch, routing_catalog_epoch) |
  lane(LaneId, lane_epoch, binding_revision, availability_epoch,
       routing_attestation_id) |
  migration(migration_generation)
local_effect_resource_version? =
  host_checkout(host_id, ProjectKey, checkout_resource_id, attestation_epoch,
                git_common_dir_resource_id, host_config_fingerprint)
prerequisite_fingerprint
claimant_id
claimant_epoch
claim_expires_at
state = planned | claimed | applying | reconciling | finalize_ready | aborting |
        published_pending_handoff | completed | aborted_compensated |
        aborted_blocked | blocked

effect_id
operation_id
ordinal
target = runtime | linear | github | git | filesystem | process | workspace_hook
kind
idempotency_scope
idempotency_key
expected_fingerprint
request_digest
desired_state_digest
publication_failure_policy?  # frozen before invocation
state = planned | claimed | invoking | observed_succeeded | observed_not_applied |
        unknown | reconciling | forward_applied | saga_finalized |
        compensating | compensated | blocked
receipt?
```

Database constraints require `PRIMARY KEY(effect_id)`,
`UNIQUE(operation_id, ordinal)`, and
`UNIQUE(idempotency_scope, target, idempotency_key)`. The idempotency scope is the
provider account/repository/workspace plus immutable target object. For create effects,
it is the immutable parent collection (for Linear, workspace+team) and the key is the
IntakeIntentId/provider marker because the target object does not yet exist. It is never
a process-local string.

Operation constraints also require `UNIQUE(parent_operation_id, continuation_kind)`,
`UNIQUE(IntakeIntentId)` for intake parents. PublishedTrackerIssueReservation requires
the matching `published_intake` occupancy. A database CHECK requires the tagged
expected_authority_version to match the operation subject/transition; registration and
project quarantine never fabricate availability. Operation and effect plans are
immutable after insert; only the current RoutingAttestation pointer in a lane expected
version may advance through `reattest_operation` while all semantic selected-binding
fields remain equal.

An effect's immutable `expected_fingerprint` covers semantic selected ProjectKey,
RepositoryKey, binding revision/fingerprint, Lane/target identity and desired provider
preconditions; local effects also seal local_effect_resource_version. It excludes
RoutingCatalog epoch and RoutingAttestation id. Effect CAS
also checks the operation's current attestation pointer, so re-attestation cannot alter
the planned target/request while allowing semantically identical catalog refresh.

Issue-resolution/routing-request/project/project-quarantine/migration-scoped operations
are allowed only for resolution, routing, registration, availability, migration/
adjudication, and intake work before Lane admission. They cannot grant lane ownership.
When issue creation succeeds, the atomic project-to-lane handoff binds the resulting
TrackerIssueKey, typed IntakeAuthority, Lane, claim, Program mapping, and admission event
through the transition kernel. `runtime.intake_commit` must invoke the same private
`LaneStore::commit_transition` primitive for every Lane, claim, epoch, or current
resource field; its surrounding transaction may write only non-lane intake records.
Lane-scoped operations then require LaneId and epoch. No intake adapter or transaction
has a second lane writer.

An effect claimant is fenced by the operation's lane epoch and a renewable claimant
epoch. Only the lowest incomplete ordinal can execute. Immediately before any provider,
Git, filesystem, process, or hook invocation, the worker persists and fsyncs `invoking`
with immutable request/desired-state digests.
A crash from `invoking` becomes `unknown`; it must run the effect-kind reconciliation
predicate before retry. A missing receipt never means the effect did not happen.

The effect executor also holds an exclusive OS process-generation lock through the full
invocation and readback. Authority-mutating Linear/GitHub effects use in-process provider clients, not `gh`
children. Local subprocess effects use supervised process groups; every descendant must
be terminated and reaped before the lock is released. An `invoking` effect is never
reassigned while that process generation or invocation group can still execute;
claimant expiry alone is insufficient. Startup may convert `invoking` to `unknown` only
after proving the prior generation and descendants are dead and the OS lock is released.

Every EffectStore state change is one SQL compare-and-swap over `(effect_id,
current_state, operation_id, claimant_epoch, expected_authority_version_digest)`.
Claim/reclaim also increments claimant epoch in a
CAS after proving the previous invocation generation dead. A zero-row update permanently
stops that worker before PONR, OS process launch, filesystem/Git mutation, or provider
I/O. Receipt insertion and the corresponding state transition share the same
AuthorityTransaction with one append-only effect event. Every claimed/invoking/
observed/unknown/reconciling/forward/compensation/finalization transition has exactly one
event under `UNIQUE(effect_id, transition_sequence)`; event insertion failure rolls back
state and receipt, and a false event cannot commit without its state. No adapter receives
its sealed invocation capability until the `invoking` CAS plus `effect_started` event
commit and are read back.

Legal forward transitions are:

```text
planned -> claimed -> invoking
invoking -> observed_succeeded -> forward_applied
invoking -> observed_not_applied -> claimed
invoking -> unknown -> reconciling
reconciling -> observed_succeeded | observed_not_applied | blocked
observed_not_applied -> claimed
forward_applied -> saga_finalized
```

The last transition is legal only inside `finalize_operation`; an executor cannot mark
one effect finalized independently.

The operation FSM is:

```text
planned -> claimed -> applying
applying -> reconciling | finalize_ready | published_pending_handoff |
            aborting | blocked
reconciling -> applying | finalize_ready | published_pending_handoff | blocked
published_pending_handoff -> applying | finalize_ready | blocked
aborting -> aborted_compensated | aborted_blocked
blocked -> reconciling | aborting
aborted_blocked -> aborting
finalize_ready -> completed
```

`blocked` and `aborted_blocked` are nonterminal authority-holding attention states and
retain `Lane.active_operation_id`; only a typed recovery transition with fresh epochs can
leave them. `completed` and `aborted_compensated` are the only terminal operation states.

`finalize_operation` is one AuthorityTransaction CAS over operation state, claimant,
Lane/project epochs, the complete effect set, and required continuation terminal state.
An intake parent cannot become finalize_ready while published occupancy/reservation or a
nonterminal continuation remains. For success it marks every
`forward_applied` effect `saga_finalized`, applies the planned Lane/claim/conflict/
resource transition through `LaneStore::commit_transition`, appends the authority event,
clears `Lane.active_operation_id`, and sets operation `completed`. Runtime-only
operations use the same path with an empty effect set explicitly classified at plan
time. Abort finalization atomically records all compensated effects, the abort event and
Lane disposition, clears the active pointer when safe, and sets
`aborted_compensated`; blocked/unknown effects retain the pointer and cannot finalize.
Failure at any finalization statement rolls back the entire transaction.

Every registered effect declares exactly one semantic class:

- `compensable`: an exact inverse is supported under the same fresh prerequisites;
- `durable_publication`: the effect publishes an immutable or conditionally removable
  artifact. A later failure preserves publication and enters roll-forward handoff; or
- `irreversible_terminal`: the final external mutation, after which only roll-forward
  is legal.

When a later ordinal fails and no durable publication has applied, the operation enters
`aborting` and compensates every `forward_applied` `compensable` effect in strict
reverse ordinal order:
`forward_applied -> compensating -> compensated | blocked`. All successful compensation
ends in `aborted_compensated`; any failure ends in `aborted_blocked`. Effects become
`saga_finalized` only when the operation's forward result is durably committed. A
`durable_publication` may be followed by projection or handoff effects. Once one has
applied, downstream failure enters `published_pending_handoff` and does not compensate
earlier ordinals. A frozen publication policy may plan a separate conditional cleanup,
but cleanup does not reclassify publication as compensation. For `git.push_new_ref`,
that policy may plan a separate leased
`git.remote_branch_delete` continuation; deletion never reclassifies the push as
compensated, and failed lease preserves the publication and rolls forward. Update-ref
publication always rolls forward. An `irreversible_terminal`
effect must be the final external ordinal; after it starts, failures are roll-forward
only. A blocked or unknown effect cannot be skipped, reordered, or converted into
terminal authority. Provider CAS or idempotency is mandatory where stale requests could
mutate authority. Unsupported semantics stop before invocation.

| Effect kind | Reconciliation and compensation rule |
| --- | --- |
| Linear create/label/state/archive/relation | Execute only when provider proves immutable create idempotency or conditional mutation CAS as applicable. Current Linear lacks both and is unsupported for these automated effects. |
| Linear/GitHub comment | Search all pages for a deterministic marker; comments are non-authoritative and are not deleted for compensation. |
| GitHub PR close | Confirm exact repository, PR, planned head, and state. Reopen only when provider support and readback prove the same PR was open at plan time; otherwise block without terminalizing. |
| GitHub merge | Require provider exact-head precondition and landing gates; merge is an irreversible point of no return and has no synthetic compensation. |
| Git/GitHub new remote ref | Always durable publication. Optional cleanup is a separate provider-CAS/force-with-lease effect; unsupported unconditional GitHub DELETE is removed. |
| Git update-ref push | Durable publication with no automatic rewind. A downstream PR-creation failure enters `published_pending_handoff` and retries or hands off using the published exact OID. |

If a provider cannot supply the conditional or compensating semantics required for an
effect kind, automated execution is unsupported and must stop before that effect.

The exhaustive v2 mutation kinds and reconciliation policies are normative in
[Lane Authority v2 effect registry](lane-authority-v2-effects.md). Adapters cannot mutate
an inventoried target except through the effect executor.

### Current Linear capability-degraded workflow

For current Linear, v2 execution starts only from a pre-existing issue accepted through
issue-batch/decision-contract intake and a provable RoutingIssueSnapshot. If label
snapshot capability is unproven, the registered project must use a provable team-only
predicate or remain quarantined; C7 does not activate an ambiguous label-routed project.

Dispatch, review, landing, closeout, supersession, attention, and cleanup authority live
entirely in Lane/ExecutionGroup/GitHub/local effect state. They do not require Linear
state, label, brief, relation, archive, or issue-create mutations. V2 removes those
tracker mutation tools from advertised agent/operator capabilities, removes queue-label
polling and mandatory closeout label/state callers, and derives scheduler/status from
Program Intake plus the private Lane ledger. The Linear issue remains user-managed.

An append-only privacy-safe Linear comment may run as a detachable projection operation
only after the internal authority transition commits. Failure creates reason-coded
projection debt/retry and cannot block or roll back Lane finalization. End-to-end C3
fixtures must complete issue-batch dispatch, review, landing, closeout, conflict release,
and cleanup with zero unsupported Linear invocation.

### Staged Supersession Authority

`RepairHandoffAuthority` is created when Decodex accepts findings and creates a repair
lane. It is immutable and contains the exact predecessor Lane/issue/PR/head, successor
Lane/issue, accepted finding ids/fingerprint, source review checkpoint, actor, and event
id. It also freezes the predecessor lane epoch. It deliberately contains no future
successor PR or merge identity.

Schema constraints allow one active handoff per `(predecessor_lane_id,
predecessor_epoch)` and one terminal SupersessionEdge per predecessor LaneId. Handoff
state is `active | replaced | cancelled | accepted | rejected_stale`. A
`replace_repair_handoff` transition CASes predecessor epoch and the active handoff,
records typed cancellation/replacement reason and successor disposition, and creates the
replacement atomically; rows are never overwritten. Generic repair-lane creation cannot
bypass this transition.

After the successor lands, `SupersessionAcceptance` references the handoff and adds exact
successor repository/PR/head/merge, default-branch reachability evidence, accepted
review/landing authority, and one `PredecessorPatchDisposition` per unique predecessor
patch:

```text
landed_in_successor { predecessor_patch_unit_digest, successor_merge, reachability_evidence }
covered_by_successor { predecessor_patch_unit_digest, successor_patch_unit_digest, review_evidence }
accepted_not_required { predecessor_patch_unit_digest, accountable_operator, independent_reviewer, reason }
```

Legacy recovery uses `OperatorSupersessionAttestation` instead of fabricating a repair
handoff. It names accountable operator and distinct reviewer principals and binds all
predecessor/successor identities plus the same patch dispositions.

Only the `accept_supersession` transition creates the immutable `SupersessionEdge`. It
requires the same ProjectKey/RepositoryKey, terminal landed successor authority,
default-branch reachability, no active predecessor operation, and complete patch
disposition. It CASes predecessor Lane epoch, active handoff id/state, and absence of a
predecessor edge in the same AuthorityTransaction that commits terminal predecessor
authority/conflict release. Concurrent losers become immutable `rejected_stale` history
and cannot close or release anything. Generic tracker relations and merged-PR facts
cannot create authority.

### Predecessor Patch Universe

`RepairHandoffAuthority` freezes repository key, target base ref and base SHA, merge-base
SHA, predecessor head SHA, and the ordered commit OIDs in
`merge_base..predecessor_head`. The canonical `decodex.patch_set/1` is computed by one
pinned in-process implementation from raw Git objects, independent of user config,
attributes, diff drivers, rename heuristics, locale, and Git text formatting.

The canonicalizer records repository object format (`sha1|sha256`) and computes every
best merge base from the raw commit DAG. Exactly one best merge base is required;
multiple best bases (including criss-cross ancestry) reject authority rather than
creating an implementation-specific virtual merge. The commit set is every commit
reachable from predecessor head and not reachable from the selected merge base. Its
order is deterministic Kahn topological order with parents before children and raw OID
bytes as the priority-queue tie breaker. Commit-object parent order remains unchanged for
merge_topology units. Any missing/shallow object rejects.

The serializer sorts paths by raw byte order and records, for each changed path, the old
and new path bytes, object kind, mode, and blob/tree/submodule OID. Renames are represented
as delete plus add, never inferred. It also records each commit OID, ordered parent OIDs,
tree OID, and explicit empty/merge classification. Canonical serialization is
length-delimited, versioned, and SHA-256 hashed.

The disposition universe contains exactly these `PatchUnit` records:

- one `path_delta` for each sorted base-to-head changed-path record;
- one `net_zero_path_history` for each raw-byte path with no endpoint delta but at least
  one non-empty commit transition, containing the deterministic commit-order sequence of
  `(commit_oid, first-parent-or-empty old mode/OID, new mode/OID)` records;
- one `empty_commit` for a root commit whose tree is the canonical empty tree, for a
  one-parent commit whose tree equals that parent, or for a merge commit whose tree
  equals its ordered first parent; equality only to a non-first merge parent is not
  empty; and
- one `merge_topology` for each merge commit, containing its ordered parents and tree.

Blob/object and ordinary commit records are canonical PatchSet evidence but are not
additional disposition units. A path modified by many commits still yields one endpoint
`path_delta`; if it returns exactly to base it instead yields one net-zero history unit.
Every PatchUnit has one SHA-256
`patch_unit_digest`, and `SupersessionAcceptance` has exactly one disposition for every
digest with no duplicates or extras. A base/head force push, merge-base change, object
change, or PatchSet digest change invalidates the handoff and requires new authority.
C4 fixtures cover merge, empty, repeated path edits, binary, rename-equivalent
delete/add, submodule, mode-only, unusual path bytes, base-change, and force-push cases.
Byte-level fixtures include root, one-parent, octopus merge, tree-equals-first-parent,
and tree-equals-only-non-first-parent cases.

### Superseded closeout operation

Acceptance creates one deterministic `SupersededCloseoutOperationId =
SHA-256("superseded-closeout/1" || SupersessionEdgeId || predecessor_epoch)` and one
ordered plan. It never reconstructs authority from comments, labels, current worktrees,
or a fresh best-guess successor. Every retry loads this same operation and rejects if the
edge, predecessor epoch, successor merge, default-branch reachability, or planned target
versions differ.

The operation stages are:

1. `acceptance_attested`: exact successor merge/reachability, predecessor PR, Lane,
   ExecutionGroup, conflict, worktree/ref/control resources, and provider version tokens
   are read fully and persisted as prerequisites;
2. `terminal_authority_committed`: one AuthorityTransaction consumes the acceptance,
   marks predecessor non-executable `terminal_cleanup_pending`, terminalizes its current
   ExecutionGroup membership, releases the exact conflict leases, creates required PR/
   local cleanup effects and optional projection debt, and advances Lane/claimant epochs;
3. `predecessor_pr_reconciled`: the exact expected-version PR-close effect reads back
   already-closed as success, conditionally closes when supported, or stops as
   capability/drift debt without reopening executable authority;
4. `resources_reconciled`: control/thread/marker, worktree, remote/local ref, and other
   owned cleanup effects run in the registry order with exact resource versions; and
5. `terminal`: required effects and readbacks have receipts, optional collaboration debt
   is detached, and the Lane moves from `terminal_cleanup_pending` to `terminal`.

The terminal-authority transaction precedes external close/comment/cleanup mutation, so a
crash cannot leave an externally closed predecessor that still owns an executable
conflict. No external effect precedes its durable planned row. Every stage and effect uses
the operation id plus fixed ordinal as idempotency key; a missing receipt is `unknown` and
must read back before retry. Cleanup failure leaves only the fenced operation claimant;
stale snapshots, a moved successor, reopened/retargeted predecessor PR, unexpected
provider version, or resource ownership drift stop the affected effect with a typed event.
Optional Linear/GitHub explanatory comments are detachable projection debt and are never
terminal or conflict authority.

## Required Transitions

- `admit`: prove ProjectBinding and typed IntakeAuthority, then create the lane and
  tracker claim.
- `dispatch`: refresh the issue and binding attestation before creating a worktree or
  lease.
- `handoff`: bind exact branch, PR, base, and head identities.
- `land`: use exact reviewed head and record merge readback.
- `supersede`: prove successor authority and drive the predecessor to terminal close.
- `cleanup`: remove owned resources only after terminal authority.
- `transfer`: consume immutable `TransferAuthority` containing source Lane/binding/epoch,
  destination current binding/availability epoch, TrackerIssueKey, fresh issue facts,
  exactly-one routing proof selecting the destination ProjectKey, resource/conflict
  disposition, accountable actor, and causation event. Source and destination must have
  no active operation; every source run, worktree, control channel, and execution
  resource must already be terminal/released or be part of the same typed terminal
  disposition. One `AuthorityTransaction` terminalizes the source as transferred,
  releases/acquires conflict leases, moves the tracker claim without an observable gap,
  creates the destination Lane/IntakeAuthority/event, and preserves source Program
  mappings as immutable history. Destination IntakeAuthority is the typed `transfer`
  variant referencing TransferAuthority and original provenance with fresh destination
  attestation. In the same transaction it marks the source Program node `transferred`
  and non-schedulable, CASes the source ExecutionGroup epoch, leaves that group
  active/draining when other nodes remain or terminalizes it when all terminal
  prerequisites hold, and creates a new destination ExecutionGroup/node/mapping for the
  destination Lane. It never rewrites a Program's project. If quarantine
  exists, only `adjudicate_quarantine(transfer_to_project)` may invoke this transfer
  plan, and the reservation clears in the same transaction.
- `quarantine`: make ambiguous authority non-executable while preserving evidence.
- `rebind`: move one stopped Lane from its admitted historical binding revision to the
  current revision after fresh exactly-one routing selects the same ProjectKey,
  immutable repository/tracker identity equality, no active operation, and lane-epoch
  CAS. It atomically updates the admitted revision and authority event without
  releasing/reacquiring the tracker claim. A different selected ProjectKey requires
  `transfer`.
- `adjudicate_quarantine`: consume one quarantine reservation using fresh exactly-one
  routing, immutable identities, no active operation in any component, occupancy epoch
  plus every present Lane/ExecutionGroup epoch CAS (no fabricated Lane for an unbound
  routing quarantine), and accountable operator plus distinct reviewer authority.
  Reservation, claim, Lane,
  Program-node and ExecutionGroup epochs, and component disposition changes commit
  atomically. `release_as_history` and `terminalize` make affected nodes non-schedulable
  and terminalize groups only when complete; `transfer_to_project` creates the typed
  destination group/mapping described above. Partial release or an unclassified group is
  forbidden.
- `adjudicate_project_quarantine`: consume one unbound legacy project partition with an
  explicit resolve-or-split mapping from every dependent source node to independently
  proven new ProjectKeys, accountable operator, distinct reviewer, global repository/
  predicate uniqueness, pending ProjectPublication batch plus contract effects, and one
  atomic batch finalization. Unmapped, multiply mapped, unattested, or partial child
  publications reject; the source quarantine is immutable history after resolution.

## Revalidation Boundaries

Before every forward invocation, retry, reconciliation read/write, and compensation,
refresh and fingerprint all applicable facts:

- current ProjectBinding revision/state/fingerprint, ProjectAvailability epoch/state,
  RoutingCatalog epoch/digest, immutable RepositoryKey readback, and global exactly-one
  routing result;
- complete double-pass RoutingIssueSnapshot plus tracker state, fully paginated relations,
  and update/version token;
- predecessor and successor PR repository, number, state, base, head, and merge state;
- successor closeout authority and default-branch reachability;
- lane epoch, active claim, run/lease/control ownership, and conflict leases.
- for every local Git/filesystem/process/hook effect, exact HostCheckoutAttestation
  resource/epoch, Git common-dir identity, checkout ownership and host config fingerprint.

Drift blocks the next effect and records a reason-coded private event. It never silently
updates the plan's prerequisites.

Admission, dispatch, and forward effects require the planned availability epoch still
be current and `active`. Reconciliation reads, controlled compensation, and terminal
cleanup may run at the same current `paused` epoch so the project can converge safely;
they cannot publish new forward state. No nonterminal Lane/operation may survive
`retired`; immutable terminal Lane history remains queryable.

## Authority Event Contract

Every authority request carries a sealed `InvocationIdentity` whose fields are private
and constructible only by trusted bootstrap/transport adapters:

```text
invocation_id, origin
authenticated_principal_kind, authenticated_principal_ref
transport_session_fingerprint, supervisor_generation
trusted_job_id?, trusted_thread_id?, trusted_automation_id?
parent_invocation_id?, nonce, authenticated_at
```

Local CLI/app identity comes from the approved identity shim plus supervisor/OS peer
context; MCP identity comes from the authenticated capability profile/session;
automation/job/thread ids come from the supervisor-pinned launcher manifest and app-server
handshake. Caller payload fields, CLI flags, issue text, environment variables, and MCP
arguments are untrusted request metadata and cannot populate or override accountable
identity. Authority mutation rejects when no trusted adapter can construct the sealed
type. Events store the InvocationIdentity fingerprint and allowlisted accountable refs;
raw credential/session material remains private and is never projected.

The supervisor is the only StateStore writer in supported v2 operation. At launcher
handshake it resolves provider account ids by authenticated token introspection, binds an
`AccountabilityRoot` to that provider account plus OS audit identity, and issues a
single-use 256-bit invocation credential through an anonymous inherited file descriptor,
never argv, environment, or filesystem. The credential MAC binds invocation id, nonce,
expiry, process id/start identity, executable SHA-256, supervisor generation, origin,
capability profile, and AccountabilityRoot. Mutation RPC consumes the nonce and verifies
OS peer credentials and exact binary hash. Direct binaries, shim bypass, replay, or a
client without that descriptor cannot open a writer or construct InvocationIdentity.

### Writer broker protocol

`decodex.authority-broker/1` is the only production mutation IPC protocol. The supervisor
creates a local Unix `SOCK_SEQPACKET` channel, verifies kernel peer pid/uid/start identity
and executable digest, consumes the one-use invocation credential during `open_channel`,
and returns a random channel generation bound to InvocationIdentity. Stream sockets,
filesystem bearer tokens, environment credentials, and ambient same-user trust are not
supported.

Every deterministic-CBOR request is one packet bounded to 1 MiB and contains
`schema, request_id, channel_generation, request_sequence, invocation_id, method,
capability, subject_kind, subject_id, expected_authority_version, idempotency_key,
payload_digest, payload`. Method is one of `resolve_issue`, `route_issue`, `plan_operation`,
`commit_transition`, `invoke_effect`, `reconcile_effect`, `compensate_effect`, or
`append_diagnostic`; the channel capability has an explicit method/subject allowlist.
Unknown fields, unknown methods, out-of-order sequences, payload/digest mismatch,
cross-subject use, or a method outside the capability reject before StateStore access.
There is at most one unacknowledged request per channel.

The broker durably stores `(InvocationIdentity, request_id, request_sequence,
idempotency_key, request_digest, result_digest, AuthorityTransaction/effect/receipt refs)`.
It sends `committed`, `duplicate_committed`, `conflict`, or `rejected` only after the
owning transaction and event-chain head are fsynced. An exact duplicate returns the
durable prior result; reuse of any request/idempotency key with different bytes is an
authority violation. Client disconnect before acknowledgement leaves the durable result
queryable but grants no new writer capability.

After client or broker crash, the supervisor may issue one resume credential only after
reading the durable invocation/channel record. Resume binds the same invocation,
executable, subject allowlist, last committed request sequence, and next expected
sequence. It first returns the prior unacknowledged result or proves it absent, then
continues; it never replays an unknown external effect as forward work. Supervisor restart
reconstructs dedupe and effect reconciliation from StateStore before accepting channels.
Malformed/truncated/duplicate packets, disconnect before/after commit, broker crash before/
after fsync/ack, client pid reuse, credential replay, and resume with stale sequence each
have required crash fixtures. The C7 kernel probe executes this exact protocol and
`commit_transition` transaction, not a generic socket echo.

Dual-accountability transitions require unequal AccountabilityRoots and distinct live
supervisor credentials; different display/provider aliases under one root are not
distinct. The operator credential cannot nominate or mint the reviewer credential.
Identity-resolution conflict or unavailable token introspection fails closed before an
authority operation is planned.

The only direct-writer exception is the exact pinned maintenance binary while the
supervisor is drained/stopped, holding the exclusive generation lock and one-use
cutover-session receipt bound to its SHA-256. Each dry-run/apply/preflight/PONR/activate
stage consumes the next signed nonce and fsyncs the advanced receipt; replay or stage
skipping rejects. Activation terminally consumes the session. It authorizes only
migration state-machine commands and cannot authorize normal authority RPC.

C1A creates one Ed25519 `HostAuthorityKey` in KeyProtector after provider/OS identity
resolution; the public key id/value is pinned in supervisor config and runtime-generation
metadata. `cutover-prepare` drains/stops writers, invokes the exact C7 binary to create the
MigrationPlan, then signs canonical deterministic-CBOR
`decodex.cutover_receipt/1` containing host/accountability root, key id, session id,
single-use nonce, stage counter, expiry, v12 generation/DB+contract digests, remote-main/
reviewed PR head/landed merge, required-check run ids, verified artifact-attestation
digest/trusted-builder identity, embedded build-info digest, exact binary SHA-256,
MigrationPlan digest and prior-stage hash. The receipt file is mode 0600 but trust comes
only from signature and pinned key.

Each maintenance stage verifies signature/all bindings, compares an fsynced cutover
session journal's latest stage/hash, consumes the nonce, obtains a fresh nonce from
KeyProtector-backed HostAuthorityKey signing, atomically rewrites/fsyncs receipt+journal,
and rejects copied/truncated/reordered/expired/replayed state. The private key is
non-exportable through Decodex APIs and has explicit create/rotate/revoke audit events;
rotation is forbidden during an active cutover session. Activation records terminal
consumption before restart.

The C7 activation binary is a CI-produced artifact, not an operator-built input. Its
read-only `build-info --json` embeds repository database id, full source commit,
`dirty=false`, Cargo.lock digest, target, compiler, and workflow source digest generated
during build. The main-push workflow for the landed merge commit runs the required full
gate, builds that exact binary, and emits an OIDC-signed DSSE/in-toto provenance
attestation binding artifact SHA-256, source repository/commit, workflow identity/ref/
digest, runner environment, and test-run/check-suite ids. Cutover verifies the
attestation against the `hack-ink/decodex` repository identity and pinned workflow policy,
reads source metadata from the binary itself, and reads successful required checks for
the exact landed merge commit from GitHub. Operator environment variables may name the
PR, downloaded artifact, and output paths; they cannot supply tested/source commit,
artifact digest, or workflow identity. Any mismatch rejects before `cutover-prepare` and
is signed into no receipt.

Minimum private event fields:

```text
schema, version, event_id, event_type
authority_generation, authority_sequence, previous_event_hash, event_hash
transition_id, correlation_id, causation_id
project_key?, project_quarantine_id?, issue_resolution_request_id?, routing_request_id?
project_binding_fingerprint?
project_availability_epoch?
routing_catalog_epoch, routing_catalog_digest
tracker_issue_key?, lane_id?
invocation_id, invocation_origin
principal_kind, principal_ref_token
accountability_root_fingerprint
job_id?, thread_id?, automation_id?
invocation_identity_fingerprint
requested_selector_kind, requested_selector_fingerprint, expected_project_key?
config_resolution_source
candidate_binding_fingerprints_and_predicate_result_enums
selected_binding, selection_reason, resolver_version
intake_id?, program_id?, contract_id?
observed_facts_fingerprint
decision, reason_codes
operation_id?, effect_id?, receipt_ref?
runtime_version, recorded_at
```

Event bytes are deterministic CBOR with domain separator
`decodex.authority-event/1`; maps use canonical key order, integers use shortest encoding,
digests/UUIDs use fixed-length byte strings, and timestamps are signed 64-bit UTC
microseconds plus `(boot_id, monotonic_nanos)` evidence. `authority_sequence`, not either
clock, orders events. `event_hash = SHA-256(domain || generation || sequence ||
previous_event_hash || canonical_event_without_event_hash)`.

The first event chains from the signed generation genesis. Every AuthorityTransaction
checks and advances one global `(generation, sequence, hash)` row while committing its
state/effect/receipt event. After SQLite commit, the supervisor signs
`(host, generation, sequence, hash, database_digest)` with HostAuthorityKey and advances
the KeyProtector protected head. A crash with DB ahead of the protected head is recoverable
only by verifying the complete suffix and signing its exact head; protected head ahead of
DB, equal sequence/different hash, broken chain, missing sequence, fork, or invalid
signature freezes mutation. Audit export includes signed genesis/head/checkpoints and the
canonical event segment. Tests rewrite, delete, truncate, reorder, fork, and replay rows
and roll the wall clock backward; each tamper freezes while a legitimate post-commit/
pre-anchor crash recovers without inventing an event.

Authority identifiers use closed bounded types before persistence or projection:

- `PrincipalRefToken` is `(namespace enum, HMAC-SHA256 stable opaque provider/OS id)`;
  display name, email, token subject text, and raw session/account ids are not stored.
- requested selectors persist only kind enum, SHA-256 fingerprint of canonical private
  bytes, and optional resolved immutable key; raw CLI text, URL, slug, query, or config
  path never enters an event.
- candidate data contains only ProjectKey, revision/fingerprint, availability epoch,
  boolean match, and closed reason enums; no predicate source text or provider object.
- `RepositoryLocator` is canonical lowercase host plus provider-validated owner/repository
  segments, each 1..100 ASCII alphanumeric/`._-`, with no userinfo, port, query, fragment,
  percent encoding, control, separator, or path traversal.
- `IssueIdentifier` is provider-normalized ASCII matching
  `[A-Z][A-Z0-9]{0,15}-[1-9][0-9]{0,11}`; unsupported providers expose an opaque digest
  instead. Branch/commit/PR fields use separately bounded provider types.
- internal `ProviderObjectRef` is `(provider enum, object-kind enum, provider-validated
  immutable id bytes)` with 1..256 bytes and no control/bidi/path/query/credential shape.
  Admin projection emits only `ProviderObjectRefToken = (provider, object-kind,
  HMAC-SHA256(id bytes))`; raw provider ids remain adapter-private.

OutputBoundary rejects overlength, invalid UTF-8, control/bidi, path-shaped, credential-
shaped, query-bearing, or provider-body-marked values rather than truncating them into a
valid-looking identifier. Privacy fixtures inject all such forms through every event and
projection field.

`issue_resolution_request_id` is present without project/TrackerIssueKey for pre-resolution
events. `routing_request_id` plus TrackerIssueKey is present without
project/binding/availability for preselection reject/overlap decisions.
`project_quarantine_id` is present instead of
project/binding/availability fields only while adjudicating an unbound legacy project
partition. Otherwise ProjectKey, binding, and availability are required. `lane_id` is
absent for routing/project-scoped operations. `tracker_issue_key` is absent only for a
project-scoped operation before issue creation returns an immutable TrackerIssueKey. The
issue-created reconciliation event binds that key, and admission events thereafter
require both fields.

Required event families include binding requested/attested/rejected, dispatch selected,
transition planned, prerequisite revalidated/drifted, effect started/succeeded/failed/
reconciled, transition committed, and lane quarantined/transferred/released.

Admission request and selection-decision events are written before Program persistence.
Program Intake and Execution Program rows carry a non-null foreign key to the committed
selection-decision event. The event and Program records commit in one SQLite transaction;
event persistence failure aborts intake with no Program or mapping rows. The original
correlation id propagates into dispatch and every resulting operation.

### Projection privacy

Projection structs are deny-by-default and reject unknown serialized fields. Their exact
allowlists are:

| Surface/schema | Allowed authority fields |
| --- | --- |
| `authority_admin/1`: local admin CLI/JSON and admin MCP | schema/version, event/transition/correlation/causation/invocation ids, issue-resolution/routing-request id or project key/project-quarantine id, binding revision/fingerprint/availability epoch, routing-catalog epoch/digest, TrackerIssueKey/LaneId, actor/principal/source enums and PrincipalRefToken, accountability-root fingerprint, selector kind/fingerprint and expected immutable key/config source enum, bounded candidate `(project_key, revision, fingerprint, availability_epoch, matched, reason_codes)`, fact fingerprint, decision/reason enums, operation/effect ids and states, receipt `(target_kind, ProviderObjectRefToken, status_class, observed_version_or_hash, timestamp)`, runtime version, timestamps. No raw selector, principal text, or provider object id. |
| `authority_operate/1`: dashboard and operate MCP | schema/version, bounded service alias, canonical RepositoryLocator, bounded IssueIdentifier, lane state, operation/effect state, allowlisted public reason enums, canonical PR URL, bounded branch, commit SHA, public trace id, timestamps. |
| `authority_observe/1`: observe MCP and public-safe status | schema/version, bounded service alias, canonical RepositoryLocator, bounded IssueIdentifier, lane state, public reason enums, canonical PR URL, bounded branch, commit SHA, public next-action enum, timestamps. |
| `authority_log/1`: ordinary logs | event id/type, bounded service alias/IssueIdentifier, operation/effect state, public reason code, runtime version, timestamp. |
| `authority_metric/1`: metrics labels | event/effect type, public reason code, service alias, result class. No issue, PR, branch, commit, principal, path, correlation, or receipt labels. |
| `authority_local_error/1`: local error/crash output | event id, bounded service alias/IssueIdentifier, typed error/reason/next-action enums, operation/effect state, provider status class, timestamp. No free-form message, response body, or raw payload. |
| `authority_agent_evidence_private/1`: persisted capsules/blockers/events | schema/version, artifact id, project key, LaneId, run/attempt ids, authority event/correlation ids, opaque agent-thread ref, worktree-resource id, cwd enum (`worktree_root|repo_root|rejected_other`), typed capsule/blocker/event/result enums, command/input/output digests, allowlisted private-evidence ref ids, runtime version, timestamps. No absolute/relative raw path, raw cwd, command text, provider body, protocol payload, environment, or free-form error. |
| `authority_forensic_export_receipt_private/1`: legacy vault export receipt | schema/version, vault object id/ciphertext digest, operator/reviewer AccountabilityRoot fingerprints, purpose/retention enums, private destination id, export digest, authority event id, timestamps. No plaintext, source/destination path, provider/protocol payload, or key material. |
| `authority_migration_private/1`: local migration report | migration id/generation, project/Lane/TrackerIssue keys, source table/row refs or opaque owned-path ids, typed quarantine/tombstone/diagnostic reasons, timestamps. No raw path or legacy payload. |
| `authority_checkpoint_public/1`: checked-in checkpoint | checkpoint enum, public issue/PR ids, commits, command ids, result/count/reason enums, timestamps. |
| `authority_collaboration_public/1`: Linear and GitHub | bounded service alias, canonical RepositoryLocator, bounded IssueIdentifier, lane/public lifecycle state, opaque public marker, public reason enums, canonical PR URL, bounded branch, commit SHA, validation-result enum, public next-action enum, timestamps. |

Secrets, credential/env-var names, protocol payloads, private evidence payloads, provider
response bodies, and hidden reasoning are forbidden on every projection. Every schema
has positive and negative fixtures for CLI text/JSON, dashboard, each MCP profile, logs,
metrics, errors/crash output, migration reports, checked-in reports, Linear, and GitHub.
A private event id is allowed only on local admin/log/error schemas.

All user-visible and persisted diagnostic output flows through a central typed
`OutputBoundary`: terminal text, CLI JSON, tracing fields, panic reports, dashboard,
MCP profiles, metrics, migration/checkpoint reports, Linear, and GitHub. Authority-path
errors carry typed codes and private evidence references rather than raw provider stderr
or response bodies. The top-level error and panic hooks render only the corresponding
allowlisted local schema.

Direct `println!`, `eprintln!`, raw `tracing::*` payloads, default `Report` rendering, and
provider stderr interpolation are forbidden in authority, effect, migration, provider,
and recovery modules outside the typed sinks. TEL-04 injects credential-shaped values,
local paths, provider bodies, and protocol markers through every error/panic/log surface;
source verification fails on unregistered direct output call sites.

The agent-evidence adapter accepts only `authority_agent_evidence_private/1` values from
OutputBoundary, serializes with deny-unknown-fields, and hashes those exact bytes before
`filesystem.evidence_artifact.write`. Existing capsule/blocker/event structs cannot be
written directly. Path and cwd observations are converted to resource ids/enums before
serialization; an unclassifiable cwd rejects evidence publication.

## Migration Contract

- Migration runs only with the daemon stopped, the supervisor generation lock exclusive,
  and a SQLite `BEGIN EXCLUSIVE` transaction held on the legacy database through atomic
  v12 path detachment.
- A guard-only prerequisite release routes every supported CLI, daemon, app, MCP,
  automation, and shim launch through a version-pinned supervisor that acquires the
  runtime-generation lock before opening StateStore. Migration acquires the exclusive
  supervisor lock. Unmanaged direct execution of obsolete binaries is unsupported.
- Dry-run and apply use the same classifier.
- `migration plan` first writes the immutable MigrationPlan containing allocated
  ProjectKeys and source/classifier/contract digests. Dry-run and apply require its exact
  digest and never allocate or regenerate keys.
- Under both locks, migration performs a full WAL checkpoint, switches the source to a
  journaled rollback-safe non-WAL state, and verifies the legacy database plus sidecar
  inodes held by the exclusive connection. It atomically renames the complete v12 state
  directory to an unpredictable journaled migration-input path and installs/fsyncs a
  tombstone directory at the canonical v12 database path before releasing the SQLite
  transaction. A process that opened the old inode cannot write while the lock is held
  and can mutate only the detached non-authoritative generation afterward; a later v12
  open fails because the expected file is a directory. The v2 database lives under a new
  generation-specific directory selected only by the signed runtime manifest and is never
  published at the v12 path. Migration then uses the SQLite
  Online Backup API into an in-memory SQLite destination, runs `integrity_check`, and
  verifies schema version plus per-table row counts and canonical logical hashes against
  the source snapshot. Preflight requires source size plus serialization overhead within
  a measured memory budget or refuses migration. `sqlite3_serialize` bytes stream directly
  into the encrypted bundle and are zeroized after encryption; no named plaintext backup
  database is created. It confirms no `-wal`/`-shm` sidecar is required and snapshots
  every registered project contract with source path, mode, old digest, generated
  ProjectKey/new digest, and bundle digest.
- Backup and legacy-evidence encryption use one `decodex.encrypted_bundle/1` format: age
  v1 X25519 streaming encryption from the maintained `age` library, canonical encrypted
  manifest, per-entry plaintext digest/size/mode metadata inside ciphertext, and outer
  ciphertext digest. A migration X25519 identity is generated once; only its recipient
  is in MigrationPlan. `KeyProtector` stores the private identity in macOS
  Security.framework Keychain or Linux Secret Service. Capability probe, create/read/
  delete roundtrip, persistence, and crash recovery must pass before plan; no file,
  environment, argv, or plaintext-key fallback exists. Unsupported hosts fail closed.
- Project identity is proven from registered project state plus repository/tracker
  evidence. Local paths alone are insufficient.
- The machine legacy inventory defines every v12 table, file, receipt, and authority
  symbol as one typed source node and labels each edge `scope_reference`,
  `execution_group_affinity`, `lane_affinity`, or `diagnostic_reference`.
  Each source-node kind records exact SQLite table/column signature or owned path
  root+pattern, every production reader, writer and path discoverer, partition rule,
  edge rule, migration disposition, quarantine rule, retention, and decoder owner.
  Schema introspection and AST/call-graph verification are closed-world: an unknown
  table/column, reader-only authority source, unlisted discoverer, or unknown file under
  a Decodex-owned runtime/project artifact namespace blocks apply. Arbitrary repository
  files outside owned namespaces are not migration sources.
  ProjectBinding/ProjectAvailability, connector state, and planning/autonomy records are
  project-scoped roots migrated exactly once; their `scope_reference` edges never spread
  lane quarantine. An unproven project root creates ProjectBindingQuarantine with no
  ProjectKey rather than a guessed binding. A Program with multiple issues is one
  explicit ExecutionGroup;
  Program/mapping edges can spread quarantine only within that group and never through
  its ProjectKey. Lease, worktree, attempt, control, lifecycle/review, protocol/private/
  Linear events, loop checkpoint, evidence, conflict, marker/guard, and closeout-receipt
  nodes use `lane_affinity` only when an independently proven run/Lane/issue key exists.
  Diagnostic references never create authority or spread quarantine.
- Every inventoried source node belongs to exactly one typed partition: proven project
  root, ProjectBindingQuarantine (including its dependent legacy nodes), ExecutionGroup,
  Lane component, or unattached diagnostic/tombstone. A node may contain
  references to other partitions without becoming a graph-union edge. Zero-partition,
  multi-partition, or forbidden edge use blocks apply. Project mismatch quarantines only
  the affected Lane component and its explicit ExecutionGroup closure; it cannot
  quarantine unrelated lanes by traversing a shared project/connector root. No
  executable dependent node can escape its propagating closure. Migration creates one
  TrackerIssueQuarantine reservation for every TrackerIssueKey in a quarantined
  ExecutionGroup closure, all referencing the same classified group evidence.
- Pre-PONR migration never rewrites, moves, truncates, or deletes inventoried marker,
  guard, receipt, control, diagnostic, or other owned source artifacts except the
  explicitly journaled legacy-agent-evidence sealing protocol below. Preserved artifact
  path, mode, size, and digest are frozen in the MigrationPlan and backup manifest; v2
  state stores typed references/quarantine dispositions while final runtime readers
  ignore legacy files. Only the database path, registered project contracts, and
  journaled legacy-agent-evidence sealing change inside the rollback unit. After PONR,
  registered retirement effects may remove inert legacy artifacts under exact
  digest/ownership checks.
- Legacy agent capsule/blocker/event files and payload-bearing private DB rows are
  privacy-sensitive migration sources. The cutover streams each exact file/payload into
  a `decodex.encrypted_bundle/1` `LegacyEvidenceVault`, then fsyncs and
  verifies ciphertext/object manifest before deleting the raw file; raw DB payloads are
  never copied into v2 tables. Original bytes, source row/path and mode are also inside
  the encrypted rollback bundle. Per-object
  `planned -> encrypted_verified -> raw_removed` stages are fsynced in the migration
  journal and belong to the rollback unit. V2 indexes only typed sanitized evidence plus
  opaque vault object id/digest; ordinary evidence, diagnose, status, MCP, search, logs,
  and public surfaces cannot discover/decrypt vault contents. Offline forensic export
  requires the exact maintenance binary, exclusive lock, distinct AccountabilityRoots,
  purpose/retention record, private destination, and authority event. Rollback decrypts
  and restores exact bytes/mode/path before releasing the lock. After PONR, vault
  retirement is a separate registered retention effect.
- Surviving conflicting rows enter quarantine with reason codes and source row
  references. A physically overwritten predecessor becomes an unrecoverable tombstone
  containing only independently proven identities and loss evidence; missing branch,
  path, or provenance values are not reconstructed.
- The migration never chooses the newest writer as the presumed owner.
- Cutover acquires the process-generation and SQLite exclusive locks, verifies all known
  legacy process identities are stopped, writes and fsyncs a migration journal, detaches
  and tombstones the v12 path as specified above, prepares and fsyncs the
  generation-specific v2 database,
  copies/fsyncs/verifies the immutable encrypted legacy database/contract/evidence backup
  bundle and evidence vault, prepares
  each new project contract in its source directory, records every old/new digest and
  temp path, atomically renames and directory-fsyncs each contract, prepares/fsyncs a
  signed tombstone metadata containing migration id/generation, detached-input inode/
  digest, and backup hash inside the tombstone directory, then atomically publishes and
  directory-fsyncs the runtime-format manifest last.
- Startup validates that journal stage, manifest generation, tombstone hash/generation,
  and v2 database metadata agree. Any partial combination freezes all runtime mutation
  and exposes only migration resume/rollback diagnosis.
- A separate fsynced point-of-no-return fence is written before the first v2 mutation
  outside the database restore unit, including Git, filesystem, process, hook, Linear,
  or GitHub effects. `maintenance rollback-status` permits restore only when the fence
  is absent. Once present, recovery is freeze-and-roll-forward.
- C7 supervisor activation/restart is a process mutation and therefore requires the
  exact pinned binary to pass `cutover-preflight` first: normal v2 database open,
  supervisor writer-broker handshake, output boundary, startup invariants and a
  rollbackable internal writer probe, with external effects and process spawning
  capability-disabled. Preflight uses the real kernel IPC boundary: the maintenance
  parent journals and launches an exact-binary probe child in a dedicated process group,
  passes the real anonymous credential FD, and verifies Unix-socket/OS peer credentials,
  PID/start identity, executable hash, nonce consumption and replay rejection. Negative
  probe children exercise altered hash, wrong peer and reused credentials. The parent
  terminates/reaps every descendant and fsyncs the receipt before success; startup after
  parent crash kills/reaps the journaled exact group before rollback. This fully
  compensable probe is a migration-protocol stage inside the restore unit, not a normal
  v2 process effect.
  Preflight failure/crash leaves the PONR fence absent and permits
  rollback. The explicit `commit-point-of-no-return` cutover stage then precedes restart.
  A crash after that fence but
  before/during restart resumes activation with the pinned binary; rollback is forbidden.
- The rollback restore unit is the immutable encrypted legacy database/contract/evidence
  backup bundle, LegacyEvidenceVault and KeyProtector handle, all registered project
  contracts, v2 database, migration and rollback journals, tombstone, and runtime-format
  manifest. The PONR fence is an
  external guard outside that unit; restore requires it to be absent and never removes
  or rewrites it. Before the fence, rollback
  advances a separate fsynced rollback journal through `prepared`, per-contract
  `contract_restoring(index)`, `contracts_restored`, per-object
  `legacy_evidence_restoring(index)`, `legacy_evidence_restored`,
  `legacy_database_restored`, `v2_selection_removed`, `directories_synced`, `verified`,
  and `complete`. Each stage is
  idempotent and validates the immutable bundle hash before acting. The backup is never
  renamed, truncated, or deleted. Rollback restores each old project contract by atomic
  decrypt to a mode-0600 planned temp, verifies digest, applies recorded uid/gid where
  permitted and `fchmod`s the exact recorded mode, fsyncs, then atomically renames and
  directory-fsyncs; it atomically decrypts a
  database temp and restores it to the legacy
  path, removes v2 selection artifacts, fsyncs every touched directory, reopens through
  SQLite, reruns integrity/logical-hash and contract-digest agreement checks, verifies
  every preserved source artifact still has its planned path/mode/size/digest, restores
  and verifies every planned raw legacy-evidence file, and only then marks complete and
  releases the supervisor lock. Startup resumes or freezes from
  every intermediate restore stage; it never treats artifact absence as successful
  restoration.
- New readers and writers become active together. There is no post-cutover dual write.
- C1-C6 releases may contain dormant v2 modules but cannot select them for host runtime
  mutation. The runtime-format selector permits only active v12 before C7 cutover and
  only active v2 afterward. C7 removes legacy runtime modules before live migration;
  only the offline read-only v12 decoder remains in the final binary.

## Adjacent Defect Contract

### No effective delta

`no_effective_delta` is an observed validation fact, not a completion signal. The kernel
classifies it with exact base/head/merge-base, raw PatchSet and name-only diff digests,
worktree status, expected-surface digest, acceptance-criteria digest, checkpoint/blocker
facts, and validation command results.

- If an independent deterministic validator proves every acceptance criterion already
  true on the admitted base and no issue-owned mutation is required, the kernel emits the
  distinct `already_satisfied` decision and may complete with no implementation effect.
  Agent prose or absence of a diff cannot produce this decision.
- Otherwise the first unblocked `no_effective_delta` observation atomically records
  `NoEffectiveDeltaRecovery(operation_id, ordinal=1, fact_digest)`, advances the same Lane
  operation from `applying` or `blocked` to the existing `reconciling` state, and creates
  exactly one continuation repair attempt carrying the complete diagnostics. Its
  idempotency key is derived from LaneId, operation id,
  validation phase, admitted base, head, expected-surface digest, and ordinal.
- An exact duplicate returns the recorded retry. A changed head or changed acceptance
  contract is drift and requires a new planned operation, not a second ordinal.
- A second no-delta result for the same recovery terminalizes the operation as
  `attention_required/no_effective_delta_unresolved`; it cannot report success or schedule
  another retry. Explicit blocker evidence follows the typed blocker transition and never
  enters this recovery.

### Manual authority and related issues

`decodex commit --manual-authority` and `decodex land --manual-authority` reject
`--related` in Clap parsing. The field is removed from `ManualCommitRequest` and
`ManualLandRequest`; builders therefore cannot accept a combination the parser allowed.
The commit-local `decodex/commit/2` record remains exactly change/authority/impact.
Issue-authorized relationships are represented only by typed Lane/Intake/Supersession
records and their authority events, never by commit-message metadata. Positive parser
fixtures prove ordinary manual authority still works; negative fixtures prove every
manual-authority/related ordering and repeated flag fails before repository/provider
readback or mutation.

## Scenario Matrix

| ID | Checkpoint | Scenario | Required result |
| --- | --- | --- | --- |
| ID-01 | C1 | Two active registrations resolve to one RepositoryKey | Reject the second binding. |
| ID-02 | C1 | Tracker issue team/identifier changes | TrackerIssueKey remains stable; eligibility drift is revalidated. |
| ID-03 | C1 | Same TrackerIssueKey appears in two legacy projects | Quarantine connected component; no active claim. |
| ID-04 | C1 | Global-key overwrite evidence exists but predecessor row is gone | Record unrecoverable tombstone; do not invent missing fields. |
| ID-05 | C1 | New binding revision is published while old lanes exist | Exactly one current revision routes; old revision remains historical for attestation. |
| ID-06 | C1 | Project pause races with dispatch/effect claim | Availability epoch CAS selects one winner; paused project admits/dispatches no new work. |
| ID-07 | C1 | Resume would overlap another active routing predicate | Global intersection rejects resume without changing availability epoch/state. |
| ID-08 | C1 | Retire/delete requested with nonterminal Lane, quarantine, operation, ExecutionGroup, or resource | Reject; no cascade deletion; historical binding and evidence remain. |
| ID-09 | C1 | Predicate contains negation/contradiction, overlaps by label logic, or upgrades schema | v1 normalizer deterministically rejects unsupported/contradictory input and detects overlap; unknown version fails closed. |
| ID-10 | C1 | Registration/revision crashes at every pending DB, contract rename/fsync, attestation, and activation statement | Binding never routes until DB/contract canonical ProjectKey+revision+digest agree; recovery deterministically resumes or blocks. |
| ID-11 | C1 | Contract content/file digests are serialized and reopened | Canonical excluded-field preimage and final-byte digest attest without self-reference or implementation-dependent iteration. |
| ID-12 | C1 | Two overlapping pending publications/resumes finalize from the same catalog epoch | One RoutingCatalog CAS wins; loser remains non-active and must replan against fresh global predicates. |
| ID-13 | C1 | All ExecutionGroup lanes/nodes terminate while historical mappings remain | Kernel CASes group to terminal only after complete prerequisites; terminal history no longer blocks project retirement. |
| ID-14 | C1 | Service alias changes or checkout moves hosts/paths | ProjectBinding/semantic fingerprint and Lane attestations remain unchanged; only alias/host-resource epochs change. |
| ID-15 | C1 | First registration finalizes or migration publishes projects | Normal registration atomically creates paused availability epoch 1; migration atomically publishes explicit availability for every binding; none is missing. |
| ID-16 | C1 | Multiple paused predicates overlap, then one resumes | Paused registration succeeds after syntax/repository checks; resume alone enters active overlap CAS and rejects if another active predicate intersects. |
| ID-17 | C2 | Issue-batch caller supplies legacy project/config/token selector or credentials resolve conflicting workspace identities | Parser rejects legacy selector; host credential bootstrap quarantines conflict; only workspace-qualified unbound resolution may create TrackerIssueKey. |
| MIG-01 | C1 | Migration lock or backup verification fails | No schema/path change. |
| MIG-02 | C1 | Crash before cutover transaction commits | Legacy runtime remains authoritative and restorable. |
| MIG-03 | C1 | Crash after cutover but before first effect | Rollback-status is allowed only with absent point-of-no-return fence. |
| MIG-04 | C1 | Old binary opens the legacy runtime path after cutover | Fail on tombstone directory; do not recreate tables or discover the generation-specific v2 path. |
| MIG-05 | C1 | Crash after each journal/detach/tombstone/database/backup/manifest filesystem operation | Startup freezes or resumes deterministically; old and v2 runtimes are never both writable. |
| MIG-06 | C1 | A supported v12 launch starts during cutover, or a process/open handle already exists | Supervisor lock blocks launch or refuses cutover before backup/path replacement. |
| MIG-07 | C1 | WAL contains committed pages and rollback restore is requested before the fence | Online Backup captures logical state; restored DB passes integrity and logical-equivalence checks. |
| MIG-08 | C1 | Crash after every rollback-journal and restore filesystem operation | Resume is idempotent from the recorded stage; immutable backup remains intact and runtime stays frozen until verified complete. |
| MIG-09 | C1 | First v2 effect is a Git/filesystem/process/hook mutation with no network call | PONR fence is durable before invocation; rollback refuses restore after the attempt. |
| MIG-10 | C1 | Legacy manual-authority closeout receipt is attached, ambiguous, or unattached | Migrate to scoped authority event only when proven; otherwise diagnostic/tombstone; old reader/writer is absent. |
| MIG-11 | C1 | Crash at every project-contract prepare/rename/fsync and rollback step | DB and every contract agree on ProjectKey/generation after resume or verified rollback; immutable bundle remains intact. |
| MIG-12 | C1 | One project has normal lanes, one misrouted lane, shared connector state, and a multi-issue Program | Shared project roots migrate once; only the affected Lane and explicit ExecutionGroup closure quarantine. |
| MIG-13 | C1 | Migration is attempted before C1A deployment, after each supported launcher deployment, or with an old process | Apply refuses until every exact launcher identity proves generation-lock use; old process/handle still blocks. |
| MIG-14 | C1 | Legacy project root has ambiguous repository/tracker identity | Create ProjectBindingQuarantine with no ProjectKey; no lane-wide graph union, routing, resume, or inferred identity. |
| MIG-15 | C1 | Migration plan/dry-run/apply are repeated or plan is replaced | Exact plan digest reuses canonical ProjectKey bytes/classification; unrecorded replacement or regenerated UUID rejects. |
| MIG-16 | C1 | Unknown authority-shaped owned file, schema column/table, path discoverer, or reader-only source exists | Closed-world inventory blocks apply until exact source-node kind and disposition are registered. |
| MIG-17 | C1 | Cutover/rollback runs with every known legacy filesystem artifact class | Non-contract/non-agent-evidence source remains byte-identical pre-PONR; evidence follows MIG-20 sealing; only registered retirement mutates later. |
| MIG-18 | C7 | Exact C7 cutover command sequence runs on v12 host or crashes at each stage | Pinned binary/receipt drains and stops v12, plan/dry-run/apply selects v2 once, activation restarts exact binary, status proves generation agreement. |
| MIG-19 | C7 | Crash occurs before/after explicit PONR and supervisor restart with v12 first on PATH | Before fence rollback may restore; after fence only pinned-binary activation rolls forward and exact-binary readbacks reject v12. |
| MIG-20 | C1 | macOS/Linux migration contains injected plaintext markers in DB/files/evidence and crashes at every bundle/vault/key stage | age-v1 bundle and supported KeyProtector verify; no named plaintext backup/intermediate exists; rollback planned temps are journaled; unsupported host refuses. |
| MIG-21 | C7 | Exact-binary normal-v2 activation preflight fails/crashes before PONR | External/process capabilities remain disabled, rollbackable writer probe leaves no residue, PONR stays absent, and rollback remains available. |
| MIG-22 | C7 | Cutover receipt is forged/copied/truncated/reordered/replayed or host key rotates mid-session | Pinned Ed25519 signature, receipt bindings, nonce/stage journal and rotation lock reject before apply/PONR. |
| MIG-23 | C1 | Rollback restores 0600 and 0644 project contracts across every stage | Decrypt temp starts 0600, digest verifies, recorded uid/gid/mode is applied/fsynced before rename, and final bytes/mode agree. |
| MIG-24 | C7 | Remote main or reviewed PR head advances after validation/before PONR | Live remote/local/PR/tested/source hashes or receipt recheck disagree and cutover fails before PONR. |
| MIG-25 | C1 | Baseline binary opens immediately before/after SQLite exclusive-lock acquisition or v12 path detachment | Before-lock writer makes cutover refuse; after-lock writer cannot mutate the inode; after-detach open fails on tombstone; only detached generation can change after release. |
| MIG-26 | C7 | Operator supplies a locally built/altered binary, fake source/tested SHA, stale attestation, or CI artifact from another workflow/repository/commit | OIDC provenance, artifact digest, embedded build-info, required-check readback, landed merge, and live main must agree without operator-supplied authority. |
| QUA-01 | C1 | Quarantined TrackerIssueKey is submitted for intake | Persistent quarantine reservation rejects readmission until typed adjudication. |
| QUA-02 | C1 | Stopped Lane requests rebind to current binding revision | Exactly-one routing, immutable identities, no active operation, and epoch CAS pass before atomic rebind. |
| QUA-03 | C1 | Quarantine adjudication races with admission or lacks a distinct reviewer | Reservation/claim/component update is atomic; stale or single-principal adjudication rejects. |
| QUA-04 | C1 | Issue routes uniquely from project A to B while transfer races or source resources remain | Rebind on A rejects; transfer rejects until disposition is complete; success moves claim/conflicts atomically and preserves source history. |
| QUA-05 | C1 | Ambiguous legacy project is resolved/split and crashes through pending contract effects | Source remains quarantined until every contract attests and one batch catalog CAS activates all paused projects/maps every node; no partial split. |
| QUA-06 | C1 | Decision-contract or issue-batch Lane transfers A to B inside multi-issue group | Destination transfer authority/group/mapping and source node/group disposition commit atomically; provenance is preserved and both projects can terminate/retire. |
| QUA-07 | C1 | Fresh issue has zero or multiple routing matches before ProjectKey selection | Unbound routing event persists; overlap atomically creates quarantine occupancy/candidate evidence without fabricated project/Lane/Program. |
| ADM-01 | C2 | `repo:pubfi-mono` issue supplied to `pubfi` | Reject before Program persistence/worktree and attribute selector/principal/candidates. |
| ADM-02 | C2 | Routing predicate has zero or multiple matches | Reject; overlap is quarantined with candidate reason codes. |
| ADM-03 | C2 | Binding revision changes after intake | Dispatch rejects and records binding drift. |
| ADM-04 | C2 | Admit races with transfer for one TrackerIssueKey | One epoch/claim wins; loser re-reads and stops. |
| ADM-05 | C2 | Valid issue batch without Decision Contract | Accept typed issue-batch authority. |
| ADM-06 | C2 | Admission-decision ledger write fails | Program Intake and mappings remain absent. |
| ADM-07 | C2 | Current Linear goal creation is requested; capable-provider fixture crashes after create/before receipt/continuation while replay or occupancy race occurs | Current Linear rejects before invocation. Provider-idempotent fixture creates once; receipt/occupancy/child converge or collision quarantines without cleanup/adoption. |
| ADM-08 | C2 | Forbidden label is beyond page one or mutates around metadata/pages in either pass | Each traversal is version-bracketed and two accepted passes match; unsupported version coverage/incomplete/torn facts reject before persistence. |
| ADM-09 | C2 | Bare/misleading identifiers exist across workspaces or caller selects a project/token first | Bare selector rejects; workspace-qualified resolution obtains immutable key through workspace directory before any ProjectKey routing. |
| EFX-01 | C1 | Two workers claim one foundational effect | Claimant epoch and OS process-generation lock fence the loser through I/O. |
| EFX-02 | C1 | Crash at every foundational effect transition | Resume one operation without duplicate authority or premature cleanup. |
| EFX-03 | C1 | Mutation outcome is unknown | Reconcile desired state before any retry. |
| EFX-04 | C3 | Comment marker exists beyond page one | Scan all pages and do not duplicate. |
| EFX-05 | C3 | PR close succeeds before receipt persistence | Read back exact PR/head, record receipt, continue same operation. |
| EFX-06 | C3 | Head changes around PR close and compensation is unavailable | Block automation; never terminalize or release conflicts. |
| EFX-07 | C3 | Point-of-no-return fence exists but DB backup lacks receipt | Roll forward; rollback tooling refuses restore. |
| EFX-08 | C3 | Stale invoking worker resumes after another claimant attempt | OS process-generation lock/provider CAS prevents the stale mutation. |
| EFX-09 | C3 | Required compensation fails or is unsupported | Effect and lane block; no ordinal advance, terminal authority, or conflict release. |
| EFX-10 | C3 | Daemon parent dies while a provider/local subprocess effect is invoking | In-process provider call becomes unknown, or supervised descendants are reaped before reassignment. |
| EFX-11 | C3 | Later ordinal fails after earlier effects were forward-applied | Earlier compensable effects run in reverse order; operation ends compensated or blocked. |
| EFX-12 | C3 | Binding revision/repository/routing uniqueness changes before a forward/retry/reconciliation invocation | Invocation rejects on prerequisite drift. |
| EFX-13 | C3 | Binding or lane epoch changes before compensation | Compensation rejects safely and operation blocks without mutating the new owner. |
| EFX-14 | C1 | Duplicate effect id, operation ordinal, or target-scoped idempotency key is inserted | Database uniqueness rejects the duplicate; no invocation is claimable. |
| EFX-15 | C3 | New-ref push succeeds and PR creation fails | Push remains durable; frozen cleanup may run a separate exact leased delete, while failed lease preserves it and enters roll-forward handoff; no false compensation. |
| EFX-16 | C3 | Existing-ref push succeeds and PR creation fails | Publication remains at exact OID in `published_pending_handoff`; no automatic rewind; PR creation reconciles/rolls forward. |
| EFX-17 | C2 | Orphan issue archive/close reconciliation races with issue update or admission | Fresh binding/issue version revalidation prevents archiving an owned or changed issue. |
| EFX-18 | C1 | Failure is injected after every AuthorityTransaction statement | No orphan Program/event/Lane, incomplete active operation/effect set, claim gap, or partial resource change is visible. |
| EFX-19 | C1 | Worker A expires, worker B reclaims, then A attempts `invoking` | Effect/lane/claimant epoch CAS updates zero rows and A stops before PONR or adapter capability. |
| EFX-20 | C1 | Baseline direct SQLite/provider/Git/process/hook/config/evidence/maintenance/filesystem mutation remains outside an adapter | AST/capability verifier fails until every production callsite is classified and capability-bound. |
| EFX-21 | C3 | PR creation or process termination is followed by failure; branch/worktree cleanup crashes at every ordinal | Durable/irreversible effects never report false compensation; cleanup resumes remote-ref, worktree, then local-ref order. |
| EFX-22 | C1 | Failure occurs after every finalize_operation statement | Transaction rollback leaves no completed Lane with active operation, partially finalized effects/resources, missing event, or replayable finalized effect. |
| EFX-23 | C3 | Remote config or soft interrupt/steer crashes before/after request consumption and response | Exact config CAS restores/blocks safely; request id accepted journal prevents duplicate delivery and reconciles result/attention. |
| EFX-24 | C1 | Registration, project quarantine, Lane, and migration operations are inserted/finalized | Tagged expected authority version matches each subject; absent availability is never fabricated. |
| EFX-25 | C3 | Current Linear create/update/archive is requested, or capable-provider CAS races a mutation | Current Linear rejects before invocation; capable fixture's immutable idempotency/conditional CAS prevents duplicate/stale mutation. |
| EFX-26 | C3 | Default fetch and credential-helper lifecycle run/crash | Fetch changes only isolated operation refs with no FETCH_HEAD; helper publish/retire is exact-owner/mode/digest/process fenced. |
| EFX-27 | C1 | AST/migration inventory encounters lane-attempt worker spawn/kill or legacy issue-claim/dispatch-lock records/files | Worker mutations are registered; legacy lock readers/writers are removed and rows/artifacts become evidence/quarantine only. |
| EFX-28 | C3 | Remote ref changes between readback/create/update/delete through Git or GitHub adapters | All creation is durable; Git cleanup requires server lease; unsupported GitHub expected-OID mutations stop before invocation. |
| EFX-29 | C3 | Lane-attempt worker crashes or claimant changes around spawn/interrupt/terminate | Exact binary/PID/start/group/generation fencing prevents detached or stale workers and reaps descendants before reassignment. |
| EFX-30 | C1 | Lane A is unknown/invoking while unrelated project B changes RoutingCatalog | Observation reconciles A, semantic-equal reattestation CAS advances attestation without changing plan, and A finalizes without forward replay. |
| EFX-31 | C1 | Ordinary v12 cycle writes provider/Git/worker/account/auth/config/usage/automation state | Every callsite is exact `v12_legacy` with replacement/removal checkpoint; C1 v12 runs without v2 rows, and v2 modules have no direct mutation. |
| EFX-32 | C1 | Failure is injected between each effect CAS, receipt, and event append | One AuthorityTransaction commits all three or none; transition sequence has no gaps, duplicates, or false events. |
| EFX-33 | C3 | Current-Linear issue-batch lane runs dispatch through cleanup | Internal Lane/GitHub/local workflow completes with zero unsupported Linear mutation; optional comment debt cannot block authority. |
| EFX-34 | C1 | Project pauses with planned/unknown operations and stale workers | Pause atomically rebases epoch/claimant to convergence-only; stale workers fail, unknown effects reconcile, and no forward publication occurs. |
| EFX-35 | C3 | Account login crashes after private workspace create, Codex spawn, auth import, or cleanup | Exact process group is reaped, secret workspace is never preserved/exposed, import rolls forward, and exact cleanup converges. |
| EFX-36 | C1 | Checkout relocates or attestation epoch changes between local effect plan/invocation | Local resource-version CAS rejects stale Git/filesystem/process/hook target and requires replan. |
| EFX-37 | C1 | Authority broker packet duplicates, changes bytes, arrives out of sequence, disconnects around fsync/ack, or resumes after client/broker crash | Exact durable request returns one result; conflicting reuse rejects; resume returns prior result or reconciles unknown effect before one ordered continuation. |
| SUP-01 | C4 | Canonical repair-handoff successor lands | Accept exact typed predecessor/successor lineage. |
| SUP-02 | C4 | Generic relation plus unrelated merged PR | Reject as insufficient supersession authority. |
| SUP-03 | C4 | Transfer requested while either lane has an active operation | Reject until operation reaches a classified terminal state. |
| SUP-04 | C4 | Quarantined lane has dependent Programs/evidence/resources | Entire component stays non-executable. |
| SUP-05 | C4 | Superseded closeout completes | Terminal authority and conflict release commit together. |
| SUP-06 | C4 | Terminal lifecycle remains in history | It does not occupy an executable conflict lease. |
| SUP-07 | C4 | Repair handoff is created before successor PR exists | Immutable handoff records only available identities; later acceptance binds landed identities. |
| SUP-08 | C4 | Cleanup fails after terminal authority/conflict release | Lane stays non-executable `terminal_cleanup_pending`; fenced cleanup retry is the only action. |
| SUP-09 | C4 | Predecessor base/head is force-pushed or includes merge, empty, binary, rename, or submodule changes | Canonical PatchSet is complete; changed digest invalidates authority; every patch unit is disposed. |
| SUP-10 | C4 | Merge/empty commits and repeated edits of one path produce PatchSet | Disposition universe has exact path-delta/empty/merge units, each with one disposition and no evidence-only object/commit duplicates. |
| SUP-11 | C4 | Two repair handoffs target the same predecessor epoch and race acceptance/replacement | One active handoff/terminal edge wins by CAS; loser is immutable replaced/cancelled/rejected-stale history and releases nothing. |
| SUP-12 | C4 | Criss-cross/multiple-best-base or sibling merge DAG is traversed in different orders | Multiple best bases reject; unique-base Kahn/raw-OID ordering emits identical PatchSet bytes. |
| SUP-13 | C4 | Non-empty path changes are fully reverted to base before predecessor head | One deterministic net-zero path-history unit records ordered transitions and receives exactly one disposition. |
| SUP-14 | C4 | Crash occurs before/after acceptance attestation or terminal-authority/conflict-release commit | Same closeout operation resumes; no external mutation precedes durable plan and no executable conflict survives terminal authority. |
| SUP-15 | C4 | Predecessor PR close succeeds before receipt or returns already closed across pagination/version refresh | Exact PR/head/version readback records one effect receipt and advances the same operation without duplicate close/comment. |
| SUP-16 | C4 | Successor merge/reachability or predecessor PR target/version drifts before close invocation | Prerequisite CAS blocks the effect with typed debt; it never reconstructs a new successor or reopens executable authority. |
| SUP-17 | C4 | Crash/failure occurs at each control, marker, worktree, remote-ref, or local-ref cleanup ordinal | Exact resource receipts resume in registry order; stale ownership cannot delete another lane's resource. |
| SUP-18 | C4 | Optional tracker/GitHub explanatory projection fails, duplicates, or is delayed after cleanup | Projection debt is detachable and idempotent; Lane terminalization and conflict release do not depend on it. |
| SUP-19 | C4 | Duplicate superseded-closeout invocation reuses operation id with same or different edge/epoch bytes | Same bytes return the durable operation; changed bytes reject as authority-key collision and release nothing. |
| TEL-01 | C5 | Replay PUB-1711 admission request | Timeline names invocation origin, principal/job, selector, candidates, resolver, selection reason. |
| TEL-02 | C5 | Each CLI/dashboard/MCP/log/report/public projection is rendered | Field allowlist passes and forbidden private fields are absent. |
| TEL-03 | C5 | Metrics and local error/crash projections render private events | Typed allowlist excludes unapproved identifiers, payloads, bodies, paths, and receipts. |
| TEL-04 | C1 | Secret/path/provider-body markers enter error, panic, log, CLI, JSON, dashboard, or MCP paths | Central typed sinks redact/reject every marker; direct output scan is empty outside sink modules before any v2 mutation ships. |
| TEL-05 | C1 | Secret/path/cwd/provider/protocol markers populate every agent capsule/blocker/event field | Deny-by-default agent-evidence serializer rejects or converts to ids/enums/digests; raw markers never reach disk. |
| TEL-06 | C1 | CLI/MCP/automation caller forges principal/job/thread/automation fields | Sealed transport-derived InvocationIdentity remains authoritative; untrusted metadata cannot override stored accountability. |
| TEL-07 | C1 | Direct binary/shim bypass, token replay, altered binary, or one AccountabilityRoot supplies operator+reviewer | Writer RPC rejects missing/mismatched/consumed credential and same-root dual accountability before operation planning. |
| TEL-08 | C1 | Migrated legacy evidence is queried through evidence/diagnose/status/MCP/search/log/public surfaces | Only sanitized typed evidence and opaque vault ids are visible; plaintext requires audited offline dual-accountable export. |
| TEL-09 | C1 | Authority event row is rewritten, deleted, truncated, reordered, forked, replayed, or wall clock rolls backward | Canonical hash chain, monotonic sequence, signature and protected head detect tamper and freeze; clock cannot reorder authority. |
| TEL-10 | C5 | Crash occurs after DB event commit but before protected-head advance, or protected head is ahead/mismatched | Valid DB suffix verifies and advances signed head exactly once; ahead/mismatch freezes and audit export proves the break. |
| ADJ-01 | C6 | Unexpected no-effective-delta checkpoint claims completion | One deterministic retry receives exact base/head/PatchSet/name-only/status/expected-surface/acceptance diagnostics. |
| ADJ-02 | C6 | `--manual-authority --related` is supplied in any ordering/repetition | Clap rejects before repository/provider readback; no request/builder contains related fields. |
| ADJ-03 | C6 | Independent validator proves issue already satisfied on admitted base with no required mutation | Distinct `already_satisfied` transition completes with criteria evidence; agent no-delta prose is insufficient. |
| ADJ-04 | C6 | First retry is replayed or returns the same no-effective-delta facts | Duplicate returns the same retry; second result terminalizes reason-coded attention and never schedules a third attempt. |

## Checkpoint Gates

Each implementation checkpoint must update
[Lane Authority v2 checkpoints](../evidence/lane-authority-v2-checkpoints.md). A green
focused test is insufficient when the checkpoint requires migration, external readback,
crash replay, privacy, or lifecycle cleanup evidence.

Advancement requires all checkpoint scenarios to pass, the listed commands to succeed,
the durable ledger to identify exact commits/PRs and results, and zero unresolved blocker
or high-severity authority objections from a fresh skeptic review.

The normative commands, fixture paths, assertions, and evidence artifacts are frozen in
[Lane Authority v2 gate manifest](lane-authority-v2-gates.md). Changing that manifest is
a scope change requiring checkpoint-ledger justification and fresh skeptic review.

| Checkpoint | Entry | Exit evidence |
| --- | --- | --- |
| C0 | User-accepted objective and clean isolated worktree. | ADR/spec/ledger/XY-1251 and exact-main launcher/mutation/legacy source inventories exist; baseline verifier, `cargo make check`, current-main containment, and whitespace gates pass; no OpenWiki-specific product/runtime surface is introduced; fresh skeptic reports no blocker/high authority gaps. |
| C1 | C0 complete. | C1I AST inventory freeze precedes C1A guard deployment, which precedes C1B; ID-01..16, MIG-01..17, MIG-20, MIG-23, MIG-25, QUA-01..07, EFX-01..03, EFX-14, EFX-18..20, EFX-22, EFX-24, EFX-27, EFX-30..32, EFX-34, EFX-36..37, and TEL-04..09 pass; dormant ProjectBinding/Catalog/Availability/LaneId, sole brokered v2 AuthorityTransaction writer/finalizer, operation/effect core, tamper-evident ledger, restore/evidence vault, trusted identity, and central OutputBoundary land; inventory remains closed-world and v2 cannot mutate a v12 host. |
| C2 | C1 landed and migrated fixtures available. | ID-17, ADM-01..09 and EFX-17 pass; project-independent immutable issue resolution, complete provider snapshots, and capable-provider receipt continuation use the C1 protocol; PUB-1711 replay shows no rejected mutation. |
| C3 | C2 landed and lane transitions use v2 identity. | EFX-04..13, EFX-15..16, EFX-21, EFX-23, EFX-25..26, EFX-28..29, EFX-33, and EFX-35 pass; degraded Linear workflow/surface removal, account login, remaining mutation adapters, worker/ref fencing, and repository test gate pass. |
| C4 | C3 landed. | SUP-01..19 and full PUB-1704/PUB-1705 fixture pass; deterministic closeout crash/replay and replacement live dry-run/readback succeed without #1073 code. |
| C5 | C4 landed. | TEL-01..03 and TEL-10 pass for every surface; diagnose/timeline/audit output, signed chain verification and metrics have exact reason-code assertions while retaining C1 TEL-04..09 boundaries. |
| C6 | C5 landed. | ADJ-01..04 pass; deterministic bounded retry/already-satisfied/terminal-attention outcomes and parser-level manual-related rejection prove positive and negative paths. |
| C7 | C1-C6 landed. | MIG-18..19, MIG-21..22, MIG-24, and MIG-26 pass; final removal/activation release eliminates legacy runtime readers/writers, attested CI artifact and exact required checks bind the landed merge, offline migration selects v2 once, and full repo/migration/replay gates, final skeptic/code review, Decodex landing, issue/PR cleanup, and authority/worktree audit all pass with no required gap. |
