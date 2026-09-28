> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile the shared Chief composer

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Compare the complete 605-line inherited/current chief_composer.rs diff against
main `8674693e30e345f58538d53a71f89c7d467f3711`. The original file hash matches
the preserved register. This batch changes no production code.

| Difference | Current owner and disposition |
| --- | --- |
| Starting and stopping | awaiting_start distinguishes a new turn from steering into an existing turn. Pending user input counts only while delivery is unclaimed. Snapshot application clears the old waiting message when no pending input remains. The primary mark shows starting/stopping state without dispatching another action. |
| Escape | The first press closes an open menu or cancels dictation. Otherwise two presses within two seconds must refer to the same current work and turn. IME composition and held Escape do not interrupt. A disconnected surface does not dispatch. |
| Interrupt delivery | chief_surface.rs routes Interrupt to request_interrupt before the message-submission queue. One command uses a fresh command identity. Readback can observe that the turn already ended; the two additional reads do not repeat the interrupt. A matching snapshot clears cancellation state. No message draft or uncertain-delivery record is consumed. |
| Unavailable service | recovery_composer retains the editable text, attachments and task references, with a text-copy control. Its submit action still reaches the existing unavailable/uncertain guard in ChiefSurface::submit. Reconnection does not send retained edits. |
| Agent settings | The attachment menu links to the existing preferences renderer. The popover uses the settings width and left alignment. This is an entry to retained optional controls, not a second settings owner. |
| Model observation | Opening a menu refreshes the source-checked model observation through its existing rate-limited reader. Display uses the catalog name or exact model ID; the removed model_label helper's guessed spelling is not a lost model selector. |
| Explicit model and reasoning | Selecting a model records intent, reconciles unsupported reasoning through the selected catalog entry, and saves the draft. Reasoning strings use the extensible protocol constructor instead of a fixed list with a High fallback. New-task reasoning can inherit native defaults through the existing creation owner. |
| Primary control and labels | PrimaryMark owns the existing send, stop, done and live states. The complete model label is ellipsized to fit. Delivery mode, attachments, task references, usage and clipboard handling remain. |
| Live voice | PR1630 restored the editor alongside voice controls. The voice toolbar replaces the normal send toolbar during a call; the draft remains editable and submission stays blocked by the existing voice guard. |
| Coverage | Both inherited composer tests remain. Added tests cover waiting-state cleanup, separate cancellation, exact-turn Escape confirmation and the transition from accepted input to a running turn. No inherited test function was removed. |

The interrupt readback is a local state observation. These source checks do not
claim that a live provider has stopped or that native cancellation races are
fully accepted. The catalog and creation helpers retain their own source and
intent contracts; this file audit does not close their shared owner reviews.

## Validation and limits

Six composer tests and seven workspace tests pass on this main revision. The
workspace cases include a rendered disconnected editor with keyboard input,
retained attachments, blocked Enter/Command-Enter, blocked cancellation, and no
dispatch after reconnection. The active-voice editable-draft regression also
passes. All 14 tests run; none are ignored. No account or provider is used.

Close only the complete chief_composer.rs file disposition. The wider workspace
diff, shared surface, signed desktop acceptance and optional-feature removal
inventory remain open. In particular, passing rendered tests is not acceptance
of the signed native-composer path. Automations remain paused.
