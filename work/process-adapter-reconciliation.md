# Process adapter reconciliation

Classification: core compatibility for existing process admission and exact history
recovery. No new product control or automatic replay is added. The fixed upstream
cutoff is `595cc91e8cbb1c2ca822d0311dcf12709410c582`.

## Restored gaps

The native thread-list response uses `nextCursor` to identify unread results.
Decodex's `ExactThreadListResult` has no continuation field. Reject a response with
an unread cursor instead of exposing its candidate set as complete. Restore both
inherited cases: a nonempty first page and an empty page with a continuation.
The restored regression fails before the guard and passes after it. Other native
history readers retain their existing pagination owners.

Restore the official provisioned CLI launcher resolver. Compare its exact bytes
and bounded package metadata before selecting `CodexCLI.app/Contents/MacOS/codex`.
The launcher bytes match the fixed upstream signing package script. Never execute
the shell launcher. An altered launcher, invalid layout or non-native target fails
existing executable validation. Relocated packages and symlinks resolve correctly.
Installed desktop discovery keeps precedence over PATH for an unqualified name.

Keep `process_bundle_snapshot.rs` as the single signed bundle-copy owner. It
replaces the original resolver module's three-resource copy with the current
bounded bundle context copy, internal relative links and static identity checks.
Restore only the original entrypoint resolver and its tests. Do not add a second
snapshot owner or weaken code-signature admission.

Restore the three inherited closing-resume assertions for observed model, provider
and directory. The existing test still retains prior native events, exact response
identity and request/response digests.

## Complete process-file disposition

The full preserved process-file diff contains these other retained changes:

- Account-bound read-only activation policy now reads workspace discovery and
  managed requirements through its existing owner. The fixed credential binding
  does not become a second credential-refresh owner.
- Chief and ordinary initialization select their own capability sets. Model-default
  and exact-thread settings reads use current typed adapters and retain interleaved
  events. Start/resume observation fields retain their current durable owner.
- Dispatch refusal classification uses the shared exact-message classifier. Prior
  native activity prevents a no-submission claim. Warning-only frames remain
  display facts, and empty config-warning details add no empty paragraph.
- Existing warning tests moved; the combined initialization/handoff test now covers
  both native warning types. The former standalone native-warning scenario is
  covered by that combined test. Scope and malformed-warning assertions remain.
- Executable discovery and signed bundle copying have dedicated current owners.
  Unix process signaling uses the canonical owned-group helper, rejects invalid
  group IDs and retains the existing ESRCH handling. Refresh tests are split out.
- Remaining differences are field order, visibility for retained history projection,
  helper signatures, test movement and the added capability/refusal regressions.

The fake server has other inherited differences, so its complete ledger row stays
open. This batch changes only its two restored continuation responses.

## Evidence and limits

The process module passes 108 tests; six native tests are opt-in. This includes the
restored list cases, relocated launcher, altered launcher/layout rejection, native
image validation, signed metadata tampering, warning retention, exact refusals,
closing receipts and process cleanup. Strict runtime lint covers all features and
targets. Python fixture syntax and diff whitespace checks pass.

The explicit installed CLI bundle test verifies the snapshot against current Codex
0.158.0-alpha.2. It proves binary identity, not notarization, release, installed
Decodex acceptance or the complete app lifecycle. No upstream setup or packaging
script was executed. Broader acceptance and automations remain unchanged; scheduled
maintenance stays paused.
