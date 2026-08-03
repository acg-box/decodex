# XY-1400 ProcessGeneration authority

## Authority and scope

This page records the checked-in implementation projection for Linear issue XY-1400.
The parent product decision is XY-1398. Product authorization V3 comment
`099fb36d-9a48-407e-abdd-80dd56d13051`, architecture skeptic receipt
`02d37443-f3cf-4c11-a462-e3da80c40481`, and implementation authorization
`a0f34b3e-033e-4106-9007-c9b21d23ae57` are the accepted issue authority. Cycle-2
review receipt `1afe3710-e9f5-4d0f-84d0-9c0732923b07` and repair authorization
`2b0937ac-48b6-40e7-b301-3d074223b1c0` correct the local protocol and lifetime
boundaries without changing V3.

V3 supersedes V2 comment `30ca36d4-8b94-4f6b-9a79-93c06561ec57`. V2 and rejected
commit `4789f7f6bcf4377ff8da02d2a781575f9ff150a3` are provenance only. Rejected
XY-1396 and XY-1397 guardian and takeover designs supply no implementation authority.

This slice adds durable fenced ProcessGeneration intent, an opaque attested launch,
exact process identity, positive-only death reconciliation, account-local quarantine,
and a narrow runtime diagnostic and control port. XY-1423 extends that same intent and
readback with non-secret account credential binding. It does not add a second ledger,
routing, account selection, RuntimeSession creation, ProviderAttempt storage, remote
authentication, UI, packaging, release, provider effects, or production dispatch.

Candidate 5 may compose one ready generation and its bounded live `FencedProcess` into
ordinary Quick Task only through the existing owners. Exact Candidate-4 staged tree
`f82b866e21f12742648023a2b468cc057afa52a1` is materially rejected and supplies no
implementation authority. Candidate-5 implementation and acceptance are pending. V23
does not gain Turn, routing, account-selection, RuntimeSession, or ProviderAttempt
authority. See the
[normative owner contract](vnext-authority.md#xy-1276-quick-task-thread-establishment).

## Owner and durable model

`ProcessSupervisor` is the only product component that writes ProcessGeneration state.
The PostgreSQL runtime role has no relation privilege on the four V23 relations. It can
execute only eight closed `SECURITY DEFINER` ProcessGeneration functions. PUBLIC has no
type, relation, or function authority. The migration identity owns the relations and
functions.

V23 adds these durable concepts:

| Concept | Contract |
| --- | --- |
| Execution epoch | An external restore authority supplies an epoch UUID and matching SHA-256 authorization digest. Runtime cannot read the digest from PostgreSQL. |
| ProcessGeneration | One immutable generation, account, initial account revision, canonical credential version and fingerprint, provider binding, exact-build account-capability profile, epoch, attested launch-manifest identity, intended boot, control kind, isolation kind, optional exact process identity, state, revision, and timestamps. It stores no credential material. |
| Death evidence | One append-only positive receipt for an exact generation and source revision. |
| Transition history | One append-only row for each revision. |

The durable states are `starting`, `ready`, `stopping`, `dead`, and
`death_unknown`. A partial process identity is invalid. A bound identity contains the
boot identity, PID, process-start identity, process-group ID, and session ID. The child
must be the leader of its process group and session.

One partial unique index permits at most one non-`dead` generation for an account.
Thus, uncertainty quarantines only that account. Quarantine is a derived projection
from a nonterminal generation. It is not another durable writer or a lease.

## Opaque attested launch

`AttestedAppServerLaunch` is a private, non-clone launch authority with no public
constructor. The existing account-launch owner constructs it only after all of these
facts agree:

- the capacity permit names the same account as the immutable account binding and carries
  a positive account revision;
- the canonical Account Service snapshot, HostCredentialStore version, credential
  fingerprint, and provider identity agree at that exact account revision;
- version and canonical generated schema come from the retained executable snapshot;
- the executable digest, derived `BuildId`, fixed `app-server --stdio` arguments, and
  exact-build capability match one accepted profile;
- the exact-build capability profile positively supports the typed
  `account/chatgptAuthTokens/refresh` callback and its response shape;
- the environment policy is clear-then-set with exact `HOME`, `PATH`, and
  `CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED=1` startup-state values;
- the exact-build profile derives one macOS private-stdio lifetime capability; and
- the working directory, protected reference snapshot, and canonical macOS execution identity
  remain inside the opaque authority.

The launch-manifest SHA-256 binds its schema, macOS canonical-suspended execution policy,
platform, control and session-isolation kinds, `BuildId`, exact image digest, command identity,
ordered arguments, working directory, complete sanitized environment, account, initial account
revision, canonical credential version and fingerprint, provider binding, and exact-build account
capability. `ProcessSupervisor` derives durable `runner_identity` from this object. Its spawn API
accepts no caller-supplied runner digest, raw `Command`, command arguments, environment, account, or
control kind. After a fresh fence, the object re-attests the retained profile and constructs the
command internally. On macOS, version and schema preflights execute the protected snapshot. The
final app-server executes the canonical image while suspended, and it resumes only after the
snapshot-rooted dynamic code identity, canonical path, session, and process group match.
Unsupported platform, argument, and image profiles reject before a profile-dependent version or
schema preflight can spawn a child.

The current accepted profile is intentionally narrow:

| Field | Accepted value |
| --- | --- |
| Platform evidence | macOS arm64 |
| Version | `codex-cli 0.146.0-alpha.9.2` |
| Executable SHA-256 | `d96ae1ca1ff6fc8587842fa04c92d3ee4d31651a811c2f89b65fcfd9c28473e2` |
| Command | canonical Codex path as both executable and argv0, suspended before user code, with fixed `app-server --stdio` arguments |
| Startup state | `CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED=1` selects `DisabledEphemeral`; no remote-control argument |
| Protocol boundary | Child stdin and stdout remain private to `ProcessSupervisor`; `FencedProcess` returns no protocol handle |
| Capability | `codex-app-server-private-stdio-disabled-ephemeral-startup-v1` |
| Account callback readiness | Accepted only after the generated schema, callback shape, and exact image preflights pass. |

The current exact image and callback receipt is
[the Codex 0.146 account-callback receipt](../evidence/xy-1422-codex-0146-account-callback.md).
For the same exact tag, Codex reads the marker at startup and selects
`DisabledEphemeral` for stdio without a remote-control argument in the upstream
[CLI selection](https://github.com/openai/codex/blob/rust-v0.146.0-alpha.9.2/codex-rs/cli/src/main.rs)
and
[remote-control transport module](https://github.com/openai/codex/blob/rust-v0.146.0-alpha.9.2/codex-rs/app-server-transport/src/transport/remote_control/mod.rs).
The marker is startup-state evidence only. It is not a permanent denial policy: the
same exact build can process an alternate-control enable RPC if a protocol writer can
send one. This pre-dispatch slice therefore returns no raw stdin, raw stdout, generic
protocol access, or protocol writer from `FencedProcess` or the retained child
capability. `ProcessSupervisor` keeps both channels private only for lifetime ownership.

No Linux executable or lifetime receipt is accepted in this source slice. Linux
identity and read-only exit-observation mechanics remain available, but generic
session and descriptor setup does not install `PR_SET_PDEATHSIG`. A future Linux
parent-death primitive must be reachable only from a separately accepted exact Linux
lifetime capability. Linux ProcessGeneration launch fails closed before its
profile-dependent preflights. Every other version, image, argument shape, environment,
or capability also fails closed. A version string or protocol `CapabilityProfile`
alone cannot mint launch authority.

MacDogfoodReady requires an exact-build account-capability receipt for the callback.
Version text, generated request types, or upstream implementation presence alone is not
proof. The current exact `codex-cli 0.146.0-alpha.9.2` profile handles the root refresh
request and response, but it mints readiness only after the exact-image, generated-schema,
and live callback preflights pass. An unsupported build or callback shape fails closed
before account launch. The private-stdio lifetime profile does not independently claim
AccountLifecycle readiness.

## Fence and child control

The launch sequence is:

1. An account-launch owner rejects an unsupported platform, image, argument, or account
   callback profile. It reads one canonical account revision plus credential
   version/fingerprint/provider binding and creates one opaque attested launch. Only an
   accepted profile can run the read-only executable preflights. No provider app-server
   can start.
2. The caller supplies only a new generation ID and external execution authorization.
3. `ProcessSupervisor` derives account, initial account revision, credential
   version/fingerprint/provider binding, runner identity, current boot, accepted control
   kind, and session isolation from trusted owners. It re-reads the Account Service and
   HostCredentialStore before the fence; any version, fingerprint, provider, or enabled
   mismatch rejects the launch.
4. PostgreSQL serializes the account, locks the active external execution epoch
   through transaction commit, and commits revision 1 in `starting`.
5. Only a fresh commit returns the non-clone `FreshProcessGenerationFence`. Replay is
   readback and cannot authorize another spawn.
6. The opaque launch constructs and spawns the protected `app-server --stdio` image as
   a new session and process-group leader. Non-stdio descriptors close on exec. Raw
   stdin and stdout stay inside the supervisor-owned child.
7. The current exact lifetime capability is macOS private stdio with best-effort EOF.
   macOS has no claimed parent-death primitive. This source has no accepted Linux
   lifetime capability and does not install `PR_SET_PDEATHSIG`.
8. The supervisor reads and persists the exact boot, PID, process-start, process-group,
   and session identity before it can mark the generation `ready`. Ready readback
   includes the immutable initial account revision, credential version/fingerprint,
   provider binding, and account-capability profile.

For the Candidate-5 Quick Task path, the establishment owner must first lock and prove
the exact prospective Turn bound in the selected V16 decision as active revision 1 under
the same Conversation and V17-created starting RuntimeSession. Account Service then reads
and compares the exact
V16-selected account readiness, revision, provider binding, credential version and
fingerprint, exact-build capability, and actual HostCredentialStore binding immediately
before spawn. Account Service cannot select or substitute an account. A mismatch stops
without another V16 decision, fallback, wake, or alternate account.

The exact ProcessGeneration create envelope is effect authority only when its durable
classification is `Fresh`. `Replayed`, `Rejected`, and uncertain or locally lost results
return typed durable readback and no spawn authority. They cannot spawn, replace, adopt,
create a successor, prepare a duplicate ProviderAttempt, or terminalize the selected Turn.
Recovery uses existing ProcessGeneration, RuntimeSession, ProviderAttempt, and Conversation
reads. It adds no ledger or recovery framework. The same no-duplicate rule applies at the
fence and ready lost-result cuts.

The daemon-local in-flight reservation prevents background reconciliation from
projecting an active fence, bind, or exact termination as restored authority. It is
per generation, so it does not serialize unrelated accounts. The opaque child retains
the original unreaped `Child` and the account capacity permit. Its owned-drop path uses
the existing bounded process-group cleanup and quarantine only while this daemon still
owns that child. It does not create restored-process authority.
The cleanup owner and reaper revoke signaling authority when a child wait reaps the
leader or returns an uncertain error. They never signal a numeric process group after
that point.

The supervisor can close its private owned channels without transferring either
handle. Closing stdin sends EOF. On macOS, EOF is only a best-effort shutdown request.
It is not death evidence and does not imply process exit, credential loss, or effect
cancellation. `decodexd` remains the only product daemon.

## Positive-only reconciliation

The closed death-evidence kinds are:

- spawn did not create a child;
- positive wait of the original owned child, followed by process-group quiescence;
- Linux pidfd exit for an exact persisted identity, followed by process-group
  quiescence;
- macOS exact kqueue `EVFILT_PROC/NOTE_EXIT`, followed by process-group quiescence;
- exact owned termination followed by positive child exit and group quiescence; and
- a current boot identity that differs from the generation boot.

PID absence, process-group absence by itself, PID or PGID reuse, timeout, lease expiry,
row absence, EOF, restart, identity mismatch, and negative search are not evidence.
They cannot transition a generation to `dead`.

On startup, PostgreSQL projects every present nonterminal generation to
`death_unknown` before replacement authority is available. Runtime performs one
positive-only reconciliation pass. The server lifecycle then owns and polls bounded
background reconciliation until shutdown. One item error does not stop later items or
unrelated work.

For a same-boot restored process, runtime first checks every persisted identity field.
On macOS, it then registers a one-shot kqueue process filter and performs a final exact
identity recheck. On Linux, it opens a pidfd and performs the same final recheck. A
mismatch or absence at either exact comparison supplies no proof. The retained witness
is read-only. Runtime does not create a `Child`, adopt, proxy, reacquire, signal,
terminate, or otherwise control the restored process.

On macOS, replacement requires an exact `NOTE_EXIT` from the retained witness and
process-group quiescence. If the process exits before witness attachment, the event is
not reconstructed. Same-boot quarantine then remains until boot change. A boot change
is positive proof that the prior boot ended.

The exact termination operation accepts only a generation whose original unreaped
`Child` remains owned by this supervisor. It rechecks durable revision and exact
identity before signaling the owned process group. It never signals a restored child
or signals after leader reap. If bounded termination cannot prove exit and group
quiescence, the generation becomes `death_unknown`; timeout is not death proof.

## Restore and continuity

The execution-epoch authorization digest comes from outside the restored database.
Runtime cannot read the epoch relation or reconstruct the digest from a generation. A
restored or replayed generation never returns a fresh spawn fence. A backup or rollback
is not launch authority.

The restore owner must prove interval quiescence or establish the accepted external
monotonic anchor or a new safe execution epoch. An ambiguous restore must not supply
the external digest. Every present nonterminal generation becomes `death_unknown`.
Row absence does not prove process death and cannot authorize reconstruction,
takeover, or launch.

Conversation continuity remains bound to persisted Codex thread identity. A
replacement can use exact-thread resume when positively supported or the accepted
Context Pack fallback when resume is positively unsupported. Process survival is not
conversation continuity. ProcessGeneration creates neither a RuntimeSession nor a
fallback.

## ProviderAttempt handoff

XY-1401 owns ProviderAttempt persistence. XY-1400 preserves this exact boundary:

- one pre-dispatch transaction must bind a prepared attempt to its consumer intent,
  accepted RuntimeSession, exact ProcessGeneration, request identity, and correlation
  or idempotency key;
- Candidate-5 initial-thread preparation must also lock and require its exact selected
  Turn to remain active revision 1 under the same Conversation and accepted
  RuntimeSession, after exact V34 thread bind;
- an unproved `dispatch_authorized` attempt becomes or remains `unknown` after lost
  supervision;
- process death, kqueue exit, boot change, EOF, timeout, restart, missing events, row
  absence, or negative search never proves `not_submitted`;
- a late positive result remains attributable to the original attempt after generation
  death;
- a replacement may reconcile the attempt but cannot replay it; and
- a successor is a distinct user-authorized effect with explicit duplicate-risk
  acknowledgement.

For Candidate 5, explicit successor remains PostgreSQL-only and non-dispatch. It cannot
be used as ProcessGeneration recovery. Replayed, rejected, or ambiguous generation state
also cannot justify the conversations owner changing the Turn to failed revision 2.

ProcessGeneration quarantine protects replacement integrity. ProviderAttempt protects
effect integrity. A macOS orphan can retain credentials and finish an in-flight effect;
this slice does not claim cancellation, credential revocation, or effect containment.

## Diagnostics and production isolation

`ServiceBootstrap` exposes independent ProcessGeneration readiness and an
authority-bound borrowed runtime control port. The port is not cloneable and cannot
escape its bootstrap owner. It provides one exact or bounded diagnostic read, one exact
positive-only reconciliation request, and one exact owned-child termination request with
an expected revision.

Diagnostics distinguish prior-boot proof, owned or in-flight supervision, pending
positive spawn non-creation, an attached positive-exit observer, an observed positive
exit with its exact kernel witness kind and current process-group quiescence, exact
same-boot presence, same-boot absence, identity mismatch, unbound identity, and
observation failure. Identity-mismatch output includes the observed boot, PID,
process-start, process-group, and session facts.

The current spawn and ready methods remain crate-private and have no caller.
`decodexd` retains `CodexAdapter::unavailable()`. Current source has no Account Service
binding at ProcessGeneration spawn and rejects all inbound child requests. It is
therefore not Slice-1 ready. The Mac dogfood implementation may add only the canonical
non-secret account/credential metadata read and typed refresh-callback gateway defined
above. No credential material enters V23 or the public protocol. Scheduler,
RuntimeSession, ProviderAttempt, remote-auth, and UI do not gain ProcessGeneration
write authority. The V22 retained-title runner remains an explicit nonproduction feature
and grants no ProcessGeneration or dispatch authority.
A future live-dispatch protocol gateway is a separate typed authority. Before dispatch
can be enabled, that gateway must source-reject `remoteControl/enable` and every other
alternate-control RPC. XY-1400 does not implement that gateway.

## Deferred XY-1400 V3 adversarial acceptance matrix

The integrated core is not frozen. XY-1400 authorizes source implementation and source
review only. The later unified gate must run this matrix against the exact committed
tree without enabling provider effects or production dispatch.

| Boundary | Deferred acceptance cases |
| --- | --- |
| Rust quality gate | Run repository-owned formatting, compile, static analysis, lint, documentation, unit, integration, and nextest gates on macOS and Linux. Add and run ProcessGeneration tests only after the integrated freeze permits test changes. |
| Migration edges | Prove clean V1-to-V23 initialization and V22-to-V23 forward upgrade. Prove immutable ledger names, versions, checksums, and supported rollback posture. |
| Manifest refreeze | Capture PostgreSQL 18 source S0, restore R1, and second restore R2. Require S0=R1 and R1=R2. Regenerate and accept complete V23 schema and configured-authority digests. Remove the temporary `process_generation` exclusion that keeps the accepted V22 digest base during this source-only slice. |
| ACL and catalog hostility | Prove the expected 81 relations, 172 functions, 70 safety functions, 147 triggers, 62 runtime-callable functions, five new enums, exact ownership, no PUBLIC authority, no runtime relation DML, no grant option, closed dependencies, hostile `search_path`, overload/default-ACL rejection, and populated restore parity. |
| State machine | Exercise every legal transition. Reject every illegal transition, combined bind/state transition, identity rewrite, partial identity, history rewrite, deletion, truncation, explicit-null or stale revision, cross-generation evidence, and malformed evidence shape. |
| Opaque launch mismatch | Prove that callers cannot mismatch runner identity, executable image, command, argv0, fixed arguments, working directory, environment, account, initial account revision, credential version/fingerprint, provider binding, BuildId, or exact-build capability. Prove that macOS preflights execute only the retained protected snapshot and that the final app-server executes only the canonical path after snapshot-rooted suspended dynamic attestation. |
| Credential binding and readback | Rotate, remove, replace, disable, or change the provider between prepare, fence, spawn, callback, and ready. Every stale combination rejects or quarantines without a second launch. Intent, manifest, V23 row, transition readback, and diagnostics agree on the canonical non-secret binding. No credential material is stored. |
| Exact-build private stdio | For each accepted profile, prove fixed `app-server --stdio`, environment clearing, the exact startup marker, absence of a remote-control argument, `DisabledEphemeral` startup selection, no raw stdin/stdout or generic protocol writer in any returned capability, and rejection of changed images, versions, arguments, environment, or capability. Prove the exact `account/chatgptAuthTokens/refresh` callback round trip and reject unknown callback methods or shapes. Unsupported profiles reject before version/schema preflight spawn. The gateway source-rejects alternate-control RPCs before enablement. |
| Fence concurrency | Exercise replay, changed-intent conflict, same-account competitors, credential rotation, provider disagreement, different-account progress, epoch retirement, account deletion, transaction abort, deadlock, serialization failure, lost commit result, and restart. Only one fresh fence can authorize spawn. |
| Crash cuts | Cut before fence, after fence before spawn, during spawn, after spawn before identity capture, after capture before bind, after bind before result, before ready, after ready before result, during stopping, after evidence insert, after generation update, and after commit with lost response. Require absence, recoverable owned authority, or account-local `death_unknown`; never permit replacement without positive death. |
| Linux identity and liveness | Prove current generic retained-title and preflight children receive session/descriptor setup without ProcessGeneration lifetime authority. Prove unsupported Linux ProcessGeneration profiles reject before version/schema preflight spawn. After an exact Linux lifetime profile is separately accepted, prove its capability-gated `PR_SET_PDEATHSIG`, parent-race closure, boot ID and `/proc/<pid>/stat` start ticks, stdio close, pidfd attachment-before-recheck, positive pidfd events, child and descendant exit, group quiescence, PID/PGID reuse, and daemon death. |
| macOS identity and liveness | Prove the versioned stable `kern.bootsessionuuid`, fail-closed quarantine for an incomparable persisted boot-identity scheme, `proc_pidinfo` start time, session leadership, EOF as best effort only, a surviving orphan, credential/effect residual risk, exact match before kqueue registration, registration before final exact recheck, exact `NOTE_EXIT`, child and descendant exit, group quiescence, and boot change. |
| Exit before macOS witness | Exit the child after daemon loss but before witness attachment. Prove that absence supplies no receipt, the account stays quarantined for the rest of the boot, and unrelated accounts continue. |
| Identity hostility | Exercise stale start identity, wrong boot, PID and PGID reuse before and after attachment, mismatched group/session, missing identity, process absence, and observation errors. None can create death evidence. |
| Same-boot isolation | Prove that one uncertain generation quarantines only its account. Continue diagnostics, repositories, reconciliation, and eligible work for unrelated accounts without starvation. |
| Restore and rollback | Restore active, retired, missing, wrong-digest, duplicated, and rollback-ambiguous epochs. Prove present nonterminal rows become `death_unknown`, database contents cannot return the external digest, replay cannot return a fresh fence, and row absence cannot authorize launch. |
| Exact termination | Prove owned versus restored behavior, revision and identity checks, TERM/KILL only while the original child is unreaped, no signal after reap, positive wait plus group quiescence, descendant persistence, timeout-to-unknown, and cancellation. Restored children always return `NotOwned`. |
| Background progress | Prove bounded paging, repeated reconciliation, idempotent evidence replay, item-error isolation, daemon-control drop, witness cleanup, and no starvation from one uncertain account. Prove restart performs observation only, without adoption or signaling. |
| Forbidden topology | Prove no guardian, wrapper authority, second daemon, adoption, proxy, takeover, reacquisition, restored-process signaling, lease-as-death, or negative-proof recovery path. |
| Conversation continuity | Prove that generation death or replacement does not change persisted thread identity. Prove exact-thread resume when positively supported and accepted fallback only when resume is positively unsupported. ProcessGeneration must create neither path. |
| ProviderAttempt handoff | Exercise late success, ambiguous submission, generation death, replacement reconciliation without replay, and a distinct acknowledged successor intent. Prove process evidence never changes an attempt to `not_submitted`. |
| Production isolation | Before Slice 1, prove no default or production caller of `spawn_fenced`, no raw ProcessGeneration protocol writer, no available Codex adapter, and no enabled dispatch flag. For Slice 1, prove the only new account paths are the canonical non-secret binding read and typed refresh callback; no credential material, alternate-control RPC, scheduler authority, RuntimeSession creation, ProviderAttempt storage, remote-auth, or UI write path enters V23. |

No formatter, compiler, build, static check, migration or SQL parser, migration
execution, test, fixture, validation wrapper, generator, service, VM, UI or
accessibility check, live Codex experiment, account operation, or provider effect ran
in XY-1400 V3. No new test or fixture is part of this candidate. The only retained
test-file edit is the smallest existing `authority.rs` manifest inventory exception
needed to describe V23. All executable acceptance remains deferred.
