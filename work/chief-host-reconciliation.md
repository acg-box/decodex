# Reconcile the shared Chief service host

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Read the complete 1,721-line inherited/current chief_host.rs diff and inspect its
current routing and source consumers. The original snapshot hash matches. This
batch changes no production code.

| Difference | Current owner and disposition |
| --- | --- |
| Actor state | Add the existing prompt-review, recap and weather cache owners. Retain the single service actor, command channel, conversation runtime and account process. Removing biased selection permits all ready actor inputs to make progress. |
| Model recovery | Move recovery coordination into the host while chief_models delegates to the retained chief_model_recovery implementation. Inspect one idle bound task per round in stable rotating order, only with an observation service and empty native event queue. Keep exact generation validation and the bounded observation timeout. |
| Archive reads | Read shared native catalog membership for the task's exact bound thread, then recheck task binding and process generation. The snapshot's root-process ownership requirement is removed from this read, so a subordinate manager can be inspected. Restore/unarchive mutations still use the coordinator's exact ownership rules. |
| Settings queries and writes | Reordered model, plugin, permission, live reviewer, app exposure and voice methods retain their dedicated owners. task_model_selection becomes model_selection; set_permission_profile becomes select_permissions. Explicit writes carry exact thread/review/selection and the original attempt key. Hooks and saved app settings use their own selection objects. |
| Native goal | Replace the removed local goal module entry with chief_native_goal::read for an exact work/thread pair. This is an observation of native goal state, not a second goal executor. |
| Prompt input and media | Resolve relative input media from the exact retained process directory, with generation, task ownership and absolute-path checks. Prompt edit/upload modules retain their separate review, immutable input and receipt owners. |
| App UI | Route bounded resource reads, source fingerprints, explicit tool reviews and confirmations to existing owners. Acknowledgement requires the exact unknown operation and reservation; it records uncertainty acknowledgement without repeating the call. |
| Native Agents | Recheck the retained catalog generation after inspection. The native reader verifies child ancestry and exact thread identity. Direct input requires an explicit native canAcceptDirectInput observation and the reviewed active turn; it steers that turn or starts an idle child turn. Local pre-write refusal is rejected, while other delivery uncertainty is not replayed. No child gains manager authority. |
| Activity detail | The removed activity_detail_source helper becomes an exact work/thread timeline-source closure. read_bound retains process/account/revision/history/work/thread checks around the read. This read no longer requires that the task was created by the root process; mutation ownership remains in the mutation owners. |
| File approval detail | Keep a retained-client exact thread/turn/file item read. The application checks the same stored request and its native connection before and after enrichment. See the explicit constraint difference below. |
| Actor commands | Normalize configured input before dispatch. Settings, prompt operations, recap, native input and existing coordinator actions keep explicit routes. Resource, archive, Guardian, answer and interrupt wrappers move inline or to named helpers without losing their identities or uncertain-result handling. |
| Review and reply | The host still rejects wrong-work, disposed and non-request inbox events before replying. request_is_live_on adds connection identity to the exact native method/parameters/RPC guard. Reusing an RPC number after reconnect does not make the old request live. Misalignment continuation retains the current review token. |
| Event and shutdown handling | Recap routes its own events and stops on connection or actor shutdown. History-edit commands avoid account rotation and wakeups; explicit prompt send has its own admission. Recap generation/cancellation do not wake queued task input. Existing voice, dictation and login expiration remain. |
| Refusal and configuration | InputNotSent clears false connection-failure notices because the durable unsent receipt already owns recovery. Steer/question errors distinguish released claims from ambiguous transport errors. Nullable initial effort uses ChiefConfig::with_optional_effort. Automation-result payload types change without enabling the paused maintainer. |
| Test and presentation helpers | Add the public-start fixture and weather cache module. All inherited test functions remain. Source inspection finds no unaccounted removed function: the ten removed names map to the routing, query renames and source closure above. |

## File-approval constraint difference

The snapshot additionally required account readiness at a credential revision
before and after reading file detail. That extra fence is not retained here.
The current operation reads evidence for an already-live native request; it does
not start work or authorize a response. Its owning application query checks the
same durable request and exact native connection on both sides of the read.
Account readiness is not a substitute for that request identity. Record this
difference explicitly rather than claiming byte-identical checks or a new
credential-revision guarantee. Response admission still checks durable disposition
and consumes the native request guard.

## Validation and limits

The fresh host suite passes 15 tests; five detail-reader cases, one file-approval
projection and one native-preview regression also pass on main 7b307c47a. All
22 tests run with none ignored. git diff --check passes. No production file changes in this
batch. Full host comparison does not prove all optional features are useful to
Decodex or establish installed-native and signed desktop acceptance. Settings
pickers, App UI, prompt editing, recap and child presentation retain their separate
optional-feature decisions and acceptance records. The shared application and
remaining inherited files still require review. Automations remain paused.
