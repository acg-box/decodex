# ProcessGeneration Authority

Status: normative current domain authority. Candidate-5 Quick Task composition is an
accepted target and remains subject to its integrated gate.

## Scope

ProcessGeneration owns durable fenced provider-process intent, opaque attested launch,
exact process identity, positive-only death reconciliation, account-local quarantine,
and a narrow runtime diagnostic/control port.

It does not own routing, account selection, RuntimeSession creation, thread creation,
ProviderAttempt state, provider effects, remote authentication, UI, packaging, release,
or product dispatch. It adds no second process ledger.

ProcessGeneration is a current domain integrity record. It is not schema history,
bootstrap authority, or a database migration mechanism. The one latest schema creates
its final relations, functions, constraints, indexes, and triggers directly.

## Owner and durable model

`ProcessSupervisor` is the only product writer. The runtime former server store identity has no
relation DML for ProcessGeneration state. It can execute only the closed
ProcessGeneration command/read functions. PUBLIC has no type, relation, or function
authority. The former server store schema owner owns the objects but is not available to normal
daemon startup.

| Concept | Contract |
| --- | --- |
| Execution epoch | An external restore authority supplies an epoch UUID and matching SHA-256 authorization digest. Runtime cannot recover the digest from former server store. |
| ProcessGeneration | One immutable generation, account, initial account revision, credential version/fingerprint, provider binding, runtime-negotiated capability profile, epoch, launch-manifest identity, intended boot, control kind, isolation kind, optional exact process identity, state, revision, and timestamps. It stores no credential bytes. |
| Death evidence | One append-only positive receipt for an exact generation and source revision. |
| Transition history | One append-only row for each accepted revision. |

The durable states are `starting`, `ready`, `stopping`, `dead`, and
`death_unknown`. A partial process identity is invalid. A complete identity contains the
boot identity, PID, process-start identity, process-group ID, and session ID. The child
must lead its process group and session.

At most one non-`dead` generation may exist for one account. Uncertainty therefore
quarantines only that account. Quarantine is a derived projection, not a lease or a
second writer.

## Opaque attested launch

`AttestedAppServerLaunch` is private, non-clone launch authority with no public
constructor. The account-launch owner creates it only after these facts agree:

- the capacity permit, immutable account binding, and positive account revision;
- Account Registry, HostCredentialStore version/fingerprint, and provider binding;
- observed version identity and canonical generated schema from the retained executable
  snapshot; the version is diagnostic identity, not a release allowlist;
- verified executable identity, derived `BuildId`, fixed `app-server --stdio` process
  arguments, and one runtime-negotiated capability profile;
- positive support for the typed account refresh callback and response shape;
- a clear-then-set environment with exact `HOME`, `PATH`, and
  `CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED=1` values;
- the accepted platform lifetime capability; and
- working directory, protected executable snapshot, and execution identity.

The launch-manifest SHA-256 binds the observed schema identity, platform execution policy,
control/session isolation, `BuildId`, image identity, command and ordered arguments,
working directory, complete sanitized environment, account, initial account revision,
credential version/fingerprint, provider binding, and runtime-negotiated account capability.

`ProcessSupervisor` derives durable runner identity from the opaque launch. Its spawn API
accepts no caller runner digest, raw command, arguments, environment, account, or control
kind. After a fresh fence, the launch re-attests the retained profile and constructs the
command internally.

On macOS, preflights execute the protected snapshot. The final app-server starts
suspended and resumes only after snapshot-rooted dynamic code identity, canonical path,
session, and process group agree. Unsupported platform, process shape, argument,
environment, or capability rejects before a provider app-server can start. The user's
Codex release/version is not pinned.

The current runtime profile is intentionally narrow only where the process boundary requires:

| Field | Accepted value |
| --- | --- |
| Platform | macOS arm64 |
| Version | Observed from the user's installed Codex at startup; not pinned |
| Executable SHA-256 | Observed and rechecked for process identity; not compared with a fixed digest |
| Command | Canonical Codex path as executable and argv0, suspended before user code, with fixed `app-server --stdio` arguments |
| Startup state | `CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED=1`; no remote-control argument |
| Protocol boundary | Child stdin/stdout remain private to `ProcessSupervisor`; `FencedProcess` returns no protocol handle |
| Capability | `codex-app-server-private-stdio-disabled-ephemeral-startup-v1` |
| Account callback | Ready only after runtime executable, generated-schema, and live callback preflights pass |

The startup marker is process startup-state evidence only. The same executable can accept
an alternate-control enable RPC if a protocol writer can send one. Therefore no raw
stdin/stdout, generic protocol access, or protocol writer can leave ProcessSupervisor.
A future live-dispatch gateway must source-reject alternate-control RPCs before product
dispatch can be accepted.

No Linux ProcessGeneration profile is accepted. Generic Linux identity/read-only exit
observation does not install `PR_SET_PDEATHSIG`. A future Linux parent-death primitive
requires a separately accepted exact lifetime capability. Unsupported Linux launch fails
before profile-dependent preflights.

## Fence and child control

The launch sequence is:

1. Account launch rejects unsupported platform, process shape, callback, or argument profile.
2. It reads the canonical account revision and credential-negative binding and creates
   the opaque launch.
3. The caller supplies only a new generation ID and external execution authorization.
4. ProcessSupervisor derives all launch facts from trusted owners and rechecks Account
   Service and HostCredentialStore immediately before the database fence.
5. former server store serializes the account, locks the active execution epoch through commit,
   and creates revision 1 in `starting`.
6. Only a fresh commit returns the non-clone `FreshProcessGenerationFence`. Replay is
   readback and cannot authorize spawn.
7. The opaque launch starts the protected child as a new session/process-group leader;
   non-stdio descriptors close on exec and raw stdio remains private.
8. ProcessSupervisor persists complete process identity before it can mark the
   generation `ready`.

The daemon-local in-flight reservation prevents background reconciliation from treating
an active fence, bind, or owned termination as restored authority. It is per generation
and does not serialize unrelated accounts.

The opaque child retains the original unreaped `Child` and account capacity permit.
Cleanup and reaper ownership revoke signaling authority when the leader is reaped or a
wait returns uncertainty. No path signals a numeric process group after that point.
Closing stdin is best-effort shutdown on macOS. It is not death evidence, credential
revocation, or effect cancellation.

## Positive-only reconciliation

The closed death-evidence kinds are:

- positive proof that spawn created no child;
- positive wait of the original owned child plus process-group quiescence;
- exact Linux pidfd exit plus process-group quiescence;
- exact macOS kqueue `EVFILT_PROC/NOTE_EXIT` plus process-group quiescence;
- exact owned termination plus positive exit and group quiescence; and
- current boot identity different from the generation boot.

PID absence, process-group absence by itself, PID/PGID reuse, timeout, lease expiry, row
absence, EOF, restart, identity mismatch, and negative search are not death evidence.
They cannot transition a generation to `dead`.

On startup, every present nonterminal generation becomes `death_unknown` before
replacement authority is available. Runtime performs one bounded positive-only pass and
continues background reconciliation until shutdown. An item error does not stop later
items or unrelated accounts.

For a same-boot restored process, runtime checks every identity field, attaches one
read-only platform witness, and performs a final exact identity recheck. Mismatch or
absence supplies no proof. Runtime does not create a `Child`, adopt, proxy, reacquire,
signal, terminate, or otherwise control a restored process.

On macOS, exact `NOTE_EXIT` and group quiescence are required. If the process exits before
witness attachment, same-boot quarantine remains until boot change. A boot change is
positive proof that the prior boot ended.

Exact termination accepts only a generation whose original unreaped child remains owned
by this ProcessSupervisor. It rechecks durable revision and identity before signaling the
owned group. If bounded termination cannot prove exit and group quiescence, the state is
`death_unknown`; timeout is not death proof.

## Restore and continuity

The execution-epoch authorization digest comes from outside the database. Runtime cannot
read it from an epoch or generation row. Restored or replayed state never returns a fresh
spawn fence. Database contents alone are not launch authority.

An authorized restore owner must prove interval quiescence, use an accepted external
monotonic anchor, or establish a new safe epoch. Ambiguous restore supplies no digest.
Every present nonterminal generation becomes `death_unknown`; row absence does not prove
death or authorize reconstruction, takeover, or launch.

Conversation continuity remains bound to persisted Codex thread identity. A replacement
may use exact-thread resume when positively supported or the accepted Context Pack path
when resume is not supported. Process survival is not Conversation continuity.
ProcessGeneration creates neither a RuntimeSession nor a continuation plan.

## ProviderAttempt handoff

ProcessGeneration protects replacement integrity. ProviderAttempt protects external
effect integrity. The handoff requires:

- one atomic pre-dispatch binding to the exact consumer intent, RuntimeSession,
  ProcessGeneration, request identity, and provider key;
- an unproved authorized attempt to remain or become `unknown` after supervision loss;
- no inference of `not_submitted` from process death, kqueue exit, boot change, EOF,
  timeout, restart, missing events, row absence, or negative search;
- late positive results to remain attributable to the original attempt;
- replacement reconciliation without replay; and
- a distinct successor effect to require new user authorization and explicit duplicate
  risk acknowledgement.

A macOS orphan can retain credentials and finish an in-flight effect. ProcessGeneration
does not claim provider cancellation, credential revocation, or effect containment.

## Candidate-5 boundary

Candidate-5 initial thread establishment adds no ProcessGeneration state or owner. Before
ProcessGeneration preparation and again before spawn, ProcessSupervisor must lock and
prove the exact routing-selected Turn remains active at revision 1 under the same
Conversation and first `starting` RuntimeSession.

Account Service repeats the exact selected Account revision, `enabled` state,
AccountLifecycle and exact-build capability, provider binding, credential version and
fingerprint, and actual HostCredentialStore binding. It cannot select another account.

The create envelope has four typed outcomes:

- `Fresh`: returns the sole non-clone spawn authority;
- `Replayed`: returns durable readback and no spawn authority;
- `Rejected`: returns durable refusal/readback and no spawn authority; and
- `Unknown`: bounded readback cannot prove a safe local result.

Only `Fresh` may spawn. Every other outcome keeps the Turn active and cannot replace,
adopt, create a successor, duplicate a ProviderAttempt, or prove pre-effect refusal.
Positive spawn noncreation is required before a fresh path can become a definite
pre-effect refusal.

ProviderAttempt preparation after thread bind must lock the same Turn as active revision
1 under the exact post-bind active RuntimeSession. ProcessGeneration never writes that
Turn or RuntimeSession.

## Diagnostics and isolation

Runtime exposes an authority-bound borrowed ProcessGeneration control port. It cannot
escape its bootstrap owner. It provides bounded diagnostics, exact positive-only
reconciliation, and exact owned-child termination with an expected revision.

Diagnostics distinguish prior-boot proof, owned/in-flight supervision, pending positive
spawn noncreation, attached exit observation, positive exit and group quiescence,
same-boot presence/absence, identity mismatch, unbound identity, and observation failure.
Credential material is not representable.

No protocol, CLI, scheduler, UI, remote-auth path, or client gains ProcessGeneration
relation or spawn authority. Live provider dispatch remains separately gated.

## Acceptance

After source freeze, validation must cover:

- fresh former server store 18 empty-target latest-schema bootstrap and second-bootstrap refusal;
- exact current catalog, ownership, ACL, dependency, function/trigger, and negative
  PUBLIC/runtime checks for ProcessGeneration objects;
- opaque-launch mismatch for every image, account, credential-negative binding, command,
  argument, environment, and capability field;
- fresh/replay/reject/unknown fences, response loss, crash cuts, concurrency, epoch
  retirement, and restart;
- macOS process identity, orphan, exit-before-witness, boot-change, group-quiescence, and
  exact owned termination behavior;
- unsupported Linux launch and any later exact Linux lifetime profile independently;
- positive-only death evidence, account-local quarantine, bounded background progress,
  and restore safety;
- Candidate-5 exact Turn and selected-account fences; and
- ProviderAttempt ambiguity handoff plus reverse production-isolation scans.

No historical upgrade, schema-ledger, or migration proof is part of acceptance.
