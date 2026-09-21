# Native glass compositing probe

This experiment tests a local native glass surface above GPUI content. It does
not connect to the daemon, load accounts, send messages, or modify the production
Decodex interface. It requires macOS 26 or later.

## Structure

- A normal GPUI window draws a scrollable striped background.
- A small GPUI panel is a native child of that window, at the normal window level.
- `NSGlassEffectView.contentView` owns the panel's real GPUI foreground view.
  A runtime assertion checks this ownership.
- The panel reuses `ComposerInput`, including its native text-input handler.
- Enter copies the draft to a local label. Nothing leaves the process.
- Regular/Clear changes the native glass style.
- Parent resize recomputes the panel frame. Native child-window ownership handles
  window movement. Closing the parent requests application shutdown.

This is a feasibility probe, not a production backend. Window creation and
compilation do not prove correct rendering, input, or lifecycle behavior.

## Build

Use the same Xcode developer directory and signing identity as the production
staging workflow. The script uses the stable compiler and a separate app ID.

```sh
scripts/macos/stage_native_glass_probe.sh
open 'target/native-glass-probe/Native Glass Probe.app'
```

Set `DECODEX_PROBE_BASELINE=1` for a plain GPUI background without a native child
panel. This control helps distinguish bridge failures from desktop-tool failures.
Do not enable `visual-capture` when assessing the interactive production renderer.

## Acceptance gates

- [ ] Scroll the stripes behind the panel. Verify local glass sampling and readable text.
- [ ] Click the input. Type Latin text and compose text with a real input method.
- [ ] Check selection, Cmd-Backspace, Shift-Enter, and local Enter submission.
- [ ] Move and resize the parent. Check panel alignment and native shadows.
- [ ] Switch applications, minimize, restore, and close. Check for orphan panels.
- [ ] Add and verify menu placement, outside dismissal, and focus restoration.
- [ ] Measure frame time and check scaling on multiple displays.

On 2026-09-20 the probe compiled and passed strict Clippy. Runtime instrumentation
reported two live windows and entered both GPUI render paths. The final build
also passed the native foreground ownership assertion. Desktop capture
returned `cgWindowNotFound` for the probe, the plain GPUI control, and the existing
Decodex app. These results do not establish a glass rendering defect or visual
acceptance. Keep all interactive gates open until directly exercised.

Track the experiment and upstream alternatives in issue #1364. Do not replace the
accepted window-level material or ship a multi-window composer before these gates
pass.

## Accepted probe and production integration

The user manually tested the probe and reported that all tested behavior matched
expectations. The probe was then closed at the user's request.

The Chief composer now uses `native_glass_panel::GlassPanel` on supported macOS.
It owns the real GPUI foreground through `NSGlassEffectView.contentView` and reuses
the existing editor, attachments, model controls, dictation, and Live controls.
Menus stay in the parent window. Global overlays temporarily use the original
in-window composer; a short settling interval preserves notification dismissal
ordering. Worker views and expanded graphs hide the native composer. Other
platforms, unavailable native glass, and Reduce Transparency use the existing
in-window composer.

Integration fixes include explicit GPUI bounds refresh after native resizing,
responder restoration, parent shortcut forwarding, and child cleanup on close.
Use a borderless normal GPUI window rather than GPUIPanel: the pinned GPUI installs
AccessKit window focus forwarding only for GPUIWindow. Do not override the
content view's accessibility children; AccessKit owns that tree.

Native checks completed in the integrated application: notification-to-composer
focus transfer, full accessibility tree, text entry, Shift-Enter, and automatic
height growth. The 213 existing GPUI tests and strict Clippy passed. Desktop capture
still intermittently reports ScreenCaptureKit error -3812 for the short native
window after resizing. Command-Backspace, menu interaction, real input-method
composition, voice, and full lifecycle acceptance remain to be completed. The
production integration is not yet fully accepted or merged. Floating toolbars
are unchanged.
