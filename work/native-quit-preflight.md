# Native quit preflight qualification

Restore the inherited `--quit-preflight` mode in the isolated native glass probe.
It uses the existing `native_quit` implementation and does not connect to the
Decodex service. No production quit or draft-storage code changes.

The complete preserved/current probe diff is now one equivalent comment change.
Close only the probe's inherited file row. The production quit owner was already
preserved exactly and is not counted again.

## Observed behavior

Build the feature-gated probe with stable Rust and the selected full Xcode SDK.
Run it with a temporary HOME and CODEX_HOME. It requests native termination twice:

1. The request handler refuses the first attempt.
2. The application remains alive and the pending-reply state clears.
3. The request handler permits the second attempt.
4. GPUI's original shutdown hook runs and the process exits with status zero.

The actual output contains these four events in order. Exactly two request
callbacks occur. Logs: `/tmp/decodex-native-quit-build.log` and
`/tmp/decodex-native-quit-preflight.log`. Strict GPUI Clippy also passes with all
features and targets: `/tmp/decodex-native-quit-clippy.log`.

## Acceptance boundary

This verifies the real AppKit deferred-termination route and preserves the
original GPUI lifecycle. It does not prove draft publication, concurrent-writer
conflict handling, menu/Dock/keyboard entry paths, relaunch, signing or the full
packaged desktop. Those remain in the final R07/R12 signed desktop pass. The probe
is developer evidence, not an added product capability. Automations stay paused.
