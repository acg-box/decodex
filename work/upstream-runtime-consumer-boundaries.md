# Remaining runtime consumer applicability

Fixed upstream cutoff: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
This closes the R09 applicability classification. It does not claim additional
runtime or desktop acceptance and does not authorize feature removal.

| Area | Disposition | Retained owner and acceptance boundary |
| --- | --- | --- |
| Analytics, Top chats and expanded account reports | Optional, unimplemented research | Existing task estimates and account profile facts remain separate consumers. [Usage scope](usage-scope-reconciliation.md) records their owners and limits. No new dashboard is required by source discovery alone. |
| Voice preferences, Live voice and dictation | Implemented optional product scope; retained behavior requires correctness | Native preference ownership and local capture, transcript and receipt owners remain. Physical audio, Bluetooth, interruption and exact late remote-caption identity remain open. [Voice mapping](prompt-voice-record-reconciliation.md). |
| Provider authentication recovery notices | Implemented optional baseline surface | The source-backed Bedrock applicability is in [provider recovery](provider-auth-recovery-history.md). Saved notices do not prove current authentication and do not authorize replay or a new login command. |
| Asynchronous freeform messages | Core observation correctness | Restored `chief/tests.rs::asynchronous_questions_and_usage_are_observed_without_completing_or_waking_work` supplies an async final_answer twice and checks question/message separation and task state. Its complete restored suite is recorded in [test owner reconciliation](chief-test-owner-reconciliation.md). It does not establish a fresh signed cross-client session. |
| External writer and task ownership | Core no-replay correctness | `chief/tests.rs::external_writer_release_requires_a_new_send` checks that a refused resume does not start a turn when the external writer releases it; a new explicit input is required. Existing native writer evidence is historical. A current combined desktop/native two-client flow remains unverified. |
| Reduced motion and VoiceOver | Core correctness while animation is retained | `apps/decodex-gpui/src/ui_motion.rs::reduced` reads NSWorkspace reduced-motion and VoiceOver state. Tween sampling and scheduling use that result. The transition unit case is not proof of a physical VoiceOver session or a complete accessibility audit. Removing animation is a separate optional choice. |
| Child-agent presentation and OS notices | Native execution with local observation/presentation | Native ownership, parent attribution and exact approval guards remain. The [native child qualification](native-child-mcp-qualification.md) keeps failed human-input handoff visible. Windows/Linux-specific behavior does not establish a local port. See [native owner boundaries](upstream-native-owner-boundaries.md). |

The review inspected the relevant current motion implementation and the freeform
and external-writer test assertions, and reconciled the complete existing owner
records linked above. It adds no fresh test-execution claim. Existing passing
fixtures retain their original scope; skipped native cases remain unqualified.

The broad R09 row is no longer an unspecified feature backlog. Remaining work is
concrete acceptance of retained product paths, optional scope decisions, and the
explicit native version limits. Ordinary native input-child interaction is the
last acceptance step under the user's instruction. Maintenance remains paused.
