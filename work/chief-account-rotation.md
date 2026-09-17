# Chief account rotation

## Behavior

Keep the current account while its observed quota permits work. Otherwise select
an enabled, lifecycle-ready account with the lowest limiting quota utilization.
Use configured routing order to break ties. Exclude accounts with a nonterminal
owned process generation. Unknown quota is not available capacity.

Pause new inbox dispatches when the current account is exhausted. Wait for all
work to become idle, retire the original process, then reconnect. Database migration
21 permits a different account only after every previous conflicting generation is
dead and the root's entire subtree is idle. Preserve native thread IDs and work
records. Do not replay an uncertain or completed turn. Capacity changes are retried
by the existing bounded recovery schedule.

The process admission guard now ignores superseded credential operations, matching
the account registry's existing lifecycle rule. An active, unsuperseded recovery
still blocks admission. This fixes an account that appeared ready after a completed
reauthentication but could not start a process because its old recovery remained
in the audit history.

Connection diagnostics retain identical-error deduplication. A changed cause closes
the previous diagnostic as superseded and creates the new one. This does not resolve
the work itself or claim that connectivity has recovered.

## Validation

- Database library: 44 passed, including cross-account admission, uncertain-work
  refusal, original thread preservation after reopening, and credential supersession.
- Runtime library: 318 passed, 1 ignored. Includes sticky selection, depleted-account
  replacement, missing quota/credential exclusion, and paused inbox preservation.
- Strict runtime/database Clippy and 16 architecture checks passed.
- Native live service: the original root automatically moved from account
  `2c58c85e-c758-43c6-914b-a571809f5466` to an available account; its new process
  reached `ready`. Thread `01a0ab10-9668-7cf0-bda4-39d9769a5537` and the original
  four work records were preserved. No test prompt was added to that conversation.

## Remaining boundary

This does not relax process-death evidence or terminate a possibly shared desktop
helper. A surviving helper in an old owned process group can still prevent recovery.
Uncertain in-flight work remains fenced; rotation is not automatic replay.
