> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Native steer receipt and visual qualification

## Current evidence

The installed Codex CLI 0.158.0-alpha.2 passes the explicit native receipt fixture
with the GPUI visual capture binary. Both live observation and cold recovery
confirm the exact work, thread, turn and client submission identity. The cold
case marks the local turn complete before recovery. Receipt recovery does not
increase the two recorded inference requests or replay input.

Restore the optional native-to-GPUI capture callback, `visual_uncertain_steer`
and `prove_steer_receipt`. The capture uses a disposable service root and its
actual typed receipt query. It starts with an uncertain submission and a later
edited draft. Both captures confirm that the exact receipt clears uncertainty
and preserves `Later draft retained.`. The capture route remains opt-in.

The two generated screenshots were inspected. Both show the retained draft and
no acceptance-unknown message. The query-only fixture deliberately has no native
history adapter or runtime source; its history and draft-service notices do not
qualify those production capabilities. This evidence is not signed desktop,
normal account, installation, or release acceptance.

## Complete file dispositions

- `chief_steer_receipts.rs`: retain exact receipt matching, command-epoch checks,
  and recovered-draft resolution. `SubmissionState` is unchanged in
  `chief_drafts.rs`; `steer_identity` is unchanged in `chief_surface.rs`, apart
  from qualified type names. Restore the missing capture helper only.
- `chief_process_native_steer_tests.rs`: restore its capture callback. Retain the
  stronger terminal-before-cold-recovery case. All original receipt, identity,
  persisted history, inference-count and no-replay assertions remain.
- `chief_native_composer.rs`: retain native-agent eligibility, panel focus and
  panel-size action forwarding. The shared composer capsule handles Escape via
  `escape_interrupt`; it replaces the removed `InterruptReply` action. The same
  current work and turn require two presses within two seconds. Menu dismissal,
  IME input, connection state and expired or changed turns retain their guards.

The shared visual-capture binary and other receipt/runtime files remain open
for their other inherited differences. These three file dispositions do not
close the complete ambiguous-dispatch or desktop acceptance groups.

## Validation

- Visual-capture binary build: pass.
- Explicit installed-native receipt test with both visual captures: pass.
- Exact receipt draft preservation GPUI test: pass for unchanged, edited and
  other-task drafts, attachments, unrelated and unavailable receipts.
- Same-current-turn two-press Escape GPUI test: pass.
- Strict runtime and GPUI Clippy, all features and all targets: pass.

Native execution remains owned by Codex. No new product feature, native retry
policy, installed configuration or automation activation is introduced.
