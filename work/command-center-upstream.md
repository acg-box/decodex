# Task overview record: current status

Restore the original source review below without changing its historical text.
It distinguishes native/TUI ownership, local input correctness and proposed
optional navigation. Historical test results do not qualify the current desktop.

The native multi-blank-session/worktree flow, TUI grouping and reverse-history
search are not established as delivered by this record. Keep them in the user's
optional applicability review, rather than treating this restored proposal as new
implementation authorization. Existing draft persistence is not proof of native
blank-session creation. R07 retains the lifecycle acceptance boundary.

Pasted tab preservation exists in the current composer. Its rendered regression
passes again in this reconciliation. External-writer behavior still needs current
combined acceptance; the original two-process probe remains historical evidence.
See [the overview reconciliation](overview-record-reconciliation.md). Automations
remain paused, including after completion.

## Original historical record

# Upstream task overview review

## Model grouping

Upstream `53ff712a48379ce8df605e292afd6046ca88ae9b` adds TUI grouping by
project, status or model. It keeps selection by task identity, sorts each model
by recent updates, uses Unknown for missing metadata, and does not infer a new
task working directory from a selected row outside project grouping.

Decodex does not embed the TUI command center. Its Chief hierarchy and project
selection own task organization. This review does not add the TUI grouping control
or its key binding. This is a presentation difference, not a claim of identical UI.
Decodex already observes native task model settings, persists them separately from
input, and shows the selected task model. Missing observations remain unknown;
they do not become a model override. Six runtime settings regressions pass in
`/tmp/decodex-1217-model-settings.log`. Full signed desktop acceptance remains open.

## Another native writer

Upstream `7efb0262d6cb62abd059835453d118255da25620` lets the TUI command center
read a frozen history snapshot after an active-writer refusal. It protects the
conversation draft, skips pending request replay and paused-goal prompts, and
requires explicit attach retry. A failed history read keeps the previous view.

Decodex reads native timeline pages directly without resuming the task. Its query
checks account, process, work and history source before and after the request.
An explicit send that encounters another writer preserves the input and records
the unavailable state. The existing coordinator can retry attachment after the
owner releases the task, but it holds the old input for a new user send. This
attachment policy differs from the TUI's manual retry; neither path replays old
input automatically. The UI's misleading saved-and-waiting text now tells the
user to send again after release.

Installed Codex 0.155.0-alpha.16 qualification passes in
`/tmp/decodex-1218-native.log` with `/tmp/decodex-1218-native.py`: two isolated
app-server processes share one fixture home; resume is refused while the owner
is active, metadata and timeline stay readable, the viewer has no loaded task,
and attach succeeds after release without an extra inference request. The
runtime held-input/new-send regression passes in
`/tmp/decodex-1218-writer-release.log`. This does not prove two full desktop apps
or all history-read failure presentations. The TUI startup-future boxing change
has no Decodex TUI embedding caller.


## Blank session lifecycle gap

Upstream `516f2780fd227a80cd9fe89488f5039245090b71` opens a native blank session
without sending a turn or interrupting another session. It retains the subscription
and draft until the first turn. Destination settings and trust apply; only explicit
permission selections carry over. Pending permission publication blocks creation.

Decodex's ordinary new-conversation action opens a local editor and creates the
native conversation on the first nonempty message. Multiple native blank sessions
with independent navigation are not implemented. Chief draft retention does not
close this gap. Complete the lifecycle through native thread creation, including
first-turn transition, source ownership, and restart behavior. Check the subsequent
worktree and startup-draft commits before implementation.

Four current GPUI/controller tests pass in `/tmp/decodex-1242-drafts.log`: recipient
draft separation, page navigation, cold draft recovery with pending questions, and
new-conversation default discovery. These tests do not prove native blank-session
creation or signed desktop interaction.

The related TUI history-search paste fix (`a505c71490885a44979df056284badbfdd75b3fb`)
and terminal viewport fix (`44b9011611e1f4213ef34bd51b33476475803a94`) have no local
TUI embedding consumer. GPUI uses its own input and scroll layout. This assessment
does not claim a local reverse-history-search feature.


Upstream `6f39a47bb3b04de4c804187bfbf55edc56939aab` adds worktree creation to
this blank-session flow. It starts from the cached remote default branch, retains
the selected subdirectory, preserves source edits, and binds only the new session.
Unclaimed clean checkouts can be removed; session startup failures report the
retained directory. Decodex has no corresponding creation entrypoint yet. Complete
this with blank-session ownership, using supported host worktree operations rather
than introducing a second Git manager. Test failure cleanup and preservation of
unrelated or dirty checkouts as well as the successful path.


## Pasted indentation

Review of upstream `7a48b95c6c399eb7986fb08b947fa2d45cb40370` exposed a local
GPUI issue: the bounded input filter removed tab characters from clipboard text.
The filter now preserves tabs and newlines while still rejecting other controls
and enforcing the byte limit. GPUI does not need the TUI's timed key-burst detector.

The clipboard regression failed before the fix in `/tmp/decodex-1259-before.log`.
Three input tests pass in `/tmp/decodex-1259-after.log`, covering tab indentation,
Unicode/CRLF input, undo/redo, IME composition, and multiline caret geometry.
Strict GPUI lint passes in `/tmp/decodex-1259-lint.log`. Signed desktop display of
tab width and full submission/restart acceptance remain unverified.

## Responsive creation draft (1434)

Upstream `1e9564fb` pumps draft edits during config loading, trust/hook review,
thread creation, old-session shutdown and attachment. New sessions skip descendant
backfill and paused-goal prompts. Successful creation restores the draft without
sending a turn; failed setup keeps the draft for a retry. In-app creation keeps
its owned request alive rather than abandoning it on startup cancellation input.
At cutoff only immediate creation uses fresh attachment presentation; later
navigation uses session lineage.

The native blank-session/worktree gap above remains open. Its local implementation
must preserve edits during asynchronous setup, retain failed drafts for explicit
retry, and bind accepted drafts only to the created session. Existing per-task
GPUI drafts do not prove this lifecycle or signed desktop behavior.
