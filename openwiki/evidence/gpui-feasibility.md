---
type: "Reference"
title: "XY-1263 GPUI feasibility evidence"
openwiki_generated: true
---

# XY-1263 GPUI feasibility evidence

Status: accepted exact-pin foundation evidence. The normalized current-main
repository-owned PID-bound accessibility harness passed a fresh repo-local direct 40/40;
the exact candidate received independent review and landed in PR #1109.

As of: 2026-07-14. Normalized candidate base:
`e4c026f8e42e64cfcaeadbbb71a0a40722212762`. Pre-normalization continuation base:
`ca893804655ede02870afc809190b7e95f9b0acb`; original merged evidence:
`054714fccc6db87390e0e48e8891ca1674dda6fe`.
Authority: [vNext authority decision](../decisions/vnext-authority.md),
[vNext authority contract](../specs/vnext-authority.md), and
[vNext gate manifest](../specs/vnext-gates.md). The Linear design baseline is planning
provenance where it agrees with those sources.

## Verdict

The replacement candidate remains at the same immutable Zed commit,
[`aeeacf5439b2d30d01e38d65d767e6f31b255ecc`](https://github.com/zed-industries/zed/commit/aeeacf5439b2d30d01e38d65d767e6f31b255ecc)
using only public upstream-supported configuration and APIs. The candidate creates the
window hidden and unfocused, lets GPUI install AccessKit's adapter, then calls
`Window::activate_window`; the AX probe uses bounded activation/focus retries and a 20 ms
keydown-to-keyup interval. Those changes are candidate repairs, not isolated causal proof.

The initial repaired probe produced 10/10 successful launches, but a later 40-launch run
produced only 38 passes. One launch failed the application/window activation decision and
one failed live keyboard input. That 38/40 negative result invalidated the earlier 10/10
stability claim. The Manager subsequently obtained an ad-hoc 40/40 at
`/tmp/decodex-gpui-cold-40.DkD3aj`; it proves the candidate behavior can remain stable over
40 launches, but the launcher selected processes without recording an exact launch-bound
PID and staged binary/probe provenance. It therefore did not close the gate.

The repository now provides `scripts/macos/run_gpui_spike_accessibility_gate.swift`. It
refuses preexisting matching processes, launches the staged bundle through `NSWorkspace`,
binds the probe to the returned PID, verifies PID uniqueness, records staged executable and
probe hashes, retains per-run probe output/status and structured summaries, and terminates
only its launched process. The first bound run exposed a harness activation race: the harness
explicitly requested a nonactivating launch, then required a separate background probe to
acquire foreground ownership. Its first run found the exact window and all expected AX
content, passed AX value mutation and `AXPress`, but never became active; the probe therefore
dispatched no keyboard event. This was not an app startup, AccessKit, or keyboard-input
failure. The harness now uses the normal activating `NSWorkspace` launch contract, waits for
the returned exact-PID application to become active, and the probe requires the application
and exact window to be active before probe-driven focus work. No acceptance retry or
assertion was removed.

An earlier repaired exact command passed 40/40 at
`target/gpui-spike/evidence/cold-launch-1783938579-82143`, before the current harness
recorded its own hash and before the current probe fingerprint. It remains provenance, not
acceptance evidence for the current exact candidate.

The pre-normalization repo-local command passed 40/40 at
`/tmp/xy1263-repo-local-direct-20260714T044200Z/gate`, but branch HEAD was based on stale
divergent history and its outer zsh status files were blank. Its tool result and all per-run
statuses were zero, so it remains technical provenance, not current-main acceptance
evidence.

After preserving and verifying the complete old candidate, the owned change was replayed
and flattened onto exact `main`/`origin/main`
`e4c026f8e42e64cfcaeadbbb71a0a40722212762`. The one fresh normalized command passed
40/40 at `/tmp/xy1263-main-e4c-direct-20260714T045950Z/gate`. All 40 runs passed exact
PID, bundle, executable hash, probe hash, harness hash, single-window identity, pre-probe
application/window activation, labels/roles, focus order, stable focus at keyboard dispatch,
AX value mutation, `AXPress`, exactly one live Unicode keyboard event, and exact-process
cleanup. All 40 PIDs were distinct; all probe stderr files were empty and all 40 per-run
probe status files contained literal `0`. The outer stage and gate status files also each
contain literal `0`. Exact SHA-256 values are
`dcabd55598169de5acbd1cb6c4d1e34f13a7d6facc6b8783710eaba90aa86faf` for the staged
executable, `849c28c6d8d18a7f6806938a8bdfc1410b5b22527db1f33e0cfa417529c5b0c0`
for the probe, and
`b479fc34166ba6f85a1bea17c88b65fd5f0d024bb1024790944ee7a20294dbe7` for the
harness. The final structured summary hashes to
`8686c8e7929c094901cef68662e247aba7f7e11c8afce281a912175e2e8004da`.
The external receipt is retained for Manager review; no unavailable raw artifact from
earlier runs is represented as checked-in evidence.

The merged 80-element no-content readback is preserved below as provenance. It was a real
observation, but not a deterministic framework result: before any spike changes, restaging
and relaunching the same pin exposed all three original content labels and `AXTextField` while
still failing focus. Bounded retries accommodate the observed lazy AccessKit activation,
but no controlled comparison proves that retries caused later success. Source inspection
separately found that default show/focus ordering violates AccessKit's adapter-construction
contract; hidden/unfocused creation is retained as the compliant configuration, but its
causal contribution to the original missing-content readback was not isolated. No newer
revision or custom bridge was needed.

The exact candidate at `de6d028405159a79f1c30a4eeebdae47481e6f25` received an
independent `NO_BLOCKING_FINDINGS` review and landed as the second parent of merge commit
`d85a808a88af96d50fb4471deb00d13f4301b07d` in PR #1109. This acceptance closes only
the isolated pinned-foundation gate. This artifact does not begin or authorize production
UI by itself.

## Provenance and dependency evidence

The selected revision was the official `zed-industries/zed` `main` HEAD returned by
`git ls-remote` at retrieval time. The commit is authored 2026-07-12T23:02:26Z. The Zed
application tag `v1.10.3` resolved to `0c54c414d522234de7298039708ffe85a116892a` and was
not selected: it is an application release tag, while this gate directly exercised the
newer current GPUI source. A moving branch, wildcard version, or unpinned Git dependency
is not accepted.

The continuation rechecked official `HEAD` and `refs/heads/main` on 2026-07-13; both still
resolved to the same commit. Therefore the selected path has **no revision, toolchain,
lockfile, license, or dependency-graph delta** from the rejected run. GPUI AccessKit
support entered Zed at `1d029c5ff5654fb1b1e8caf4462993c8ee13a133`. Zed itself currently
uses `Application::new_inaccessible` unless `ZED_EXPERIMENTAL_A11Y=1`, but the standalone
`gpui_platform::application()` used here constructs the accessible application path.

The pinned source declares `gpui` 0.2.2 and `gpui_platform` 0.1.0. Both declare
Apache-2.0. GPUI's `LICENSE-APACHE` is a symlink whose link payload hashes to
`9cff568488e9d56aae60f162404acdb15d8533a0202cf01f6eb0843b264f20ff`; its resolved Apache
license text hashes to `752daf2fb234ca4a1fa372c073fe127f44b7b90fd2529ae44273a64f9d53da7a`.
The Zed repository
states that its source is primarily GPL-3.0-or-later with Apache-2.0 components where
marked and checks third-party dependencies with `cargo-about`. The resolved metadata has
664 all-target/dev packages and the macOS normal graph has 447 unique `cargo tree` lines.
Two internal crates, `gpui_shared_string` and `gpui_util`, omit Cargo license metadata;
their repository provenance is pinned but this remains a machine-readable license gap.
No Zed UI component crate or source file is copied into Decodex. The small text input is
Decodex spike code derived from the documented Apache-2.0 input API pattern.

`spikes/gpui/Cargo.lock` pins every resolved package. Git sources additionally pin Zed,
Zed font-kit `94b0f28166665e8fd2f53ff6d268a14955c82269`, proptest
`3dca198a8fef1b32e3a66f1e1897c955b4dc5b5b`, and scap
`4afea48c3b002197176fb19cd0f9b180dd36eaac`. Provenance and SPDX metadata are evidence,
not a legal opinion; distribution still needs the repository's normal notice/source
compliance review.

## Supported environment and experiments

Hardware: Apple M4 Max MacBook Pro, 16 cores, 64 GiB RAM. Software: macOS 27.0 build
26A5378j, Xcode 27.0 build 27A5209h, macOS SDK 27.0, arm64, Rust/Cargo 1.97.0. The exact
Rust toolchain is pinned in the spike. Xcode's matching 27A5209h Metal Toolchain component
is a build prerequisite; the first build correctly failed when it was absent, then passed
after `xcodebuild -downloadComponent MetalToolchain`.

| Probe | Result |
| --- | --- |
| `cargo +1.97.0 check --manifest-path spikes/gpui/Cargo.toml --all-targets` | Pass. |
| `cargo +1.97.0 test --manifest-path spikes/gpui/Cargo.toml --release` | 4 passed, 0 failed, 1 measurement ignored. |
| `workspace_headless_frame_benchmark` (240 frames), normalized current-main rerun | p50 292 µs, p95 364 µs, max 461 µs; 64 messages/2 MiB resident; 120 graph nodes. |
| `render_probe` real Metal (120 redraws), normalized current-main rerun | p50 318 µs, p95 427 µs, max 494 µs; 2560×1600 capture with 653 unique RGBA colors. |
| `scripts/macos/stage_gpui_spike.sh` | Release `.app` staged; `plutil` clean; hardened runtime ad-hoc signature passes strict verification. |
| packaged launch + `CGWindowListCopyWindowInfo` | One on-screen `Decodex GPUI Feasibility` window, 1062×713 points. |
| unchanged original spike rerun | Same pin/package exposed 249 elements, all three original labels, and `AXTextField`; focus was still missing, so the old probe exited 2. This falsified a deterministic no-content conclusion before repair. |
| initial repaired accessibility probe, ten cold launches | 10/10 reported pass, including content labels/roles, focus order, AX value/action paths, and live Unicode keyboard input. This was later invalidated as a stability claim by the 38/40 run. |
| later 40-launch probe | **Negative, 38/40:** one application/window activation-decision failure and one live-keyboard failure. Raw artifacts are not available in the repository. |
| Manager ad-hoc run at `/tmp/decodex-gpui-cold-40.DkD3aj` | 40/40 behavioral pass, but no exact launch PID plus staged binary/probe provenance; insufficient to close the gate. |
| earlier PID-bound gate, exact staged 40-launch command | **40/40 pass:** `target/gpui-spike/evidence/cold-launch-1783938579-82143`; retained as pre-current-fingerprint provenance. |
| pre-normalization repo-local PID-bound gate | **40/40 technical provenance:** `/tmp/xy1263-repo-local-direct-20260714T044200Z/gate`; stale divergent base and blank outer receipts prevent current-main acceptance. |
| normalized current-main PID-bound gate, exact staged 40-launch command | **Accepted 40/40 pass:** `/tmp/xy1263-main-e4c-direct-20260714T045950Z/gate`; 40 distinct exact PIDs, literal-zero outer and per-run receipts, and all provenance, activation, AX, focus, action/value, single-keyboard-event, and cleanup assertions passed. Exact-candidate review returned `NO_BLOCKING_FINDINGS`; PR #1109 landed the reviewed candidate. |

The measured redraw samples cover GPUI layout/paint submission for a real Metal-backed
offscreen window. They are not end-to-end display-present latency and are not a production
performance guarantee. Re-run on supported release hardware and capture presented-frame
metrics before using a 16.7 ms interactive budget as accepted product evidence.

## Large-history result and limits

The fixture represents 3,221,225,472 logical bytes as 98,304 messages of 32 KiB each.
Pages contain 64 messages (2 MiB of string payload); LRU capacity is four pages (8 MiB of
string payload). The counter uses `String::len()` and therefore excludes container,
capacity, allocator, and preview-allocation overhead; process RSS is reported separately.
The UI owns only a logical count and a paged provider. GPUI `uniform_list` requests the
visible range; initial layout caused one page/64 messages to materialize, not 98,304
messages.

`history_bench` made 128 evenly spaced probes through the entire logical history. Every
run finished with 128 page misses, 8,192 messages generated cumulatively, four resident
pages, and an exact 8,388,608-byte cached string-payload peak. On the normalized
current-main candidate, five executions took 76,252–77,479 µs internally (median
76,894 µs); `/usr/bin/time -l` reported 16,728,064–16,760,832 bytes maximum RSS. The
deterministic test asserts that cache bytes never
exceed 8 MiB and generated messages remain below total history.

This proves bounded paging/rendering for uniformly sized preview rows and synthetic fixed-
size messages. It does not prove variable-height production conversation layout, artifact
fetching, cache persistence, server cursors, search, or multi-GB database behavior; those
remain owned by XY-1295 and regression owner XY-1300.

## Feasibility coverage

- Lifecycle/event loop: `gpui_platform::application()`, `Application::run`, real
  `App::open_window`, async executor update, activation, and timed quit all executed.
- Input/focus/keyboard: `EntityInputHandler`, UTF-8/UTF-16 conversion, focus handles,
  committed Unicode text through deterministic `simulate_input`, left/backspace/end
  actions, and visible caret passed. The packaged app additionally accepted live CGEvent
  Unicode after AX focus transfer and exposed the resulting value through AX.
- Scrolling/virtualization: `uniform_list` owns scrolling and requested only the visible
  range; the paged provider enforced its independent 8 MiB string-payload bound.
- Graph: a real Metal frame contains a bounded 120-node canvas. This proves basic canvas
  composition, not production edge routing, animation, or graph-scale culling.
- Accessibility: labels/roles reached live macOS AX after lazy activation. The probe
  found workspace/history/input/clear labels, associated the labeled input with
  `AXTextField` and the labeled clear control with `AXButton`, found focused content, and the
  composer's current value. Tab and Shift-Tab moved input → clear → input; the probe also
  repeated that order through AX focus requests, set the AX value, invoked the clear
  button's `AXPress`, observed the value clear, then delivered and read back live Unicode
  keyboard input. The later 38/40 result shows those decisions were not stable under the
  then-used launcher. The repaired bound harness reproduced 40/40 with exact launch
  provenance and a pre-probe activation assertion.
- Deterministic tests: GPUI `TestAppContext`/`VisualTestContext` simulate input, action
  dispatch, async work, layout, and bounded rendering without display timing.
- Packaging/signing: the executable is a conventional `Contents/MacOS` app payload with
  `Info.plist`, macOS 14 minimum, hardened runtime, and an injectable signing identity.
  Release automation must use an Apple Development/Distribution identity, timestamp,
  notarize, staple, archive, and verify exactly as the existing Swift app boundary does.
- Menubar boundary (inherited architecture constraint, not exercised by this spike): the
  separate SwiftUI menubar stays a separate process and bundle. It must launch/activate the
  GPUI bundle and consume the same restricted versioned protocol; there is no Rust/Swift
  in-process bridge, shared database, direct rollout access, or second mutation path. GPUI
  settings reach it only through `decodexd`.

## Original rejected configuration, selected path, and update policy

The original failure remains immutable provenance. Its revision and dependency inputs did
not change; only the falsified lifecycle/probe assumptions changed:

1. Candidate Zed revision `aeeacf5439b2d30d01e38d65d767e6f31b255ecc`, Rust 1.97.0, macOS 14+, Metal,
   Xcode/Metal Toolchain, and the checked-in lockfile were the tested inputs.
2. The rejected configuration used default `WindowOptions` (`show: true`, `focus: true`).
   GPUI installed `accesskit_macos::SubclassingAdapter` only after the native window had
   already been shown/focused, contrary to AccessKit's documented construction contract.
3. The rejected probe performed one immediate traversal. AccessKit activates on the first
   children/focus/hit-test query; GPUI returns an initial root and schedules a later refresh,
   so a single traversal can complete before the content tree is published.
4. The selected configuration uses public `WindowOptions { show: false, focus: false }`,
   then public `Window::activate_window()` after `open_window` returns. The probe performs
   bounded retries after activation and tests real focus/action/value/keyboard paths. The
   current candidate also waits 20 ms between Unicode keydown and keyup. Neither retries nor
   that interval may be credited for success without a controlled comparison.
5. Conversation data enters the UI through cursor/page adapters and bounded disposable
   caches. A project or conversation open never accepts an eager all-history API.
6. The graph and history renderer receive already bounded visible slices; GPUI is not a
   product-state or paging authority.
7. SwiftUI menubar and GPUI are protocol clients of `decodexd`, never peers sharing state.

Every future GPUI evaluation uses an explicit revision, never a branch move or wildcard.
Update one commit at a time in an isolated branch, regenerate the lockfile, diff
GPUI/platform API and license metadata, run the complete XY-1263 command set, compare
history and frame evidence, and require live AX content roles/labels plus focus/action
behavior before review may accept the candidate. Keep the original rejected configuration
and 80-element readback recorded even after this exact-pin replacement is accepted.

## Falsifiers and downstream stop conditions

The technical accessibility falsifier is closed for the isolated pinned foundation by the
normalized current-main 40/40 bound run, exact literal-zero receipts, independent
`NO_BLOCKING_FINDINGS` review, and landed PR #1109. Production qualification remains
separate.
The remaining conditions continue to apply to every later evaluation:

- the exact revision/toolchain/Metal component cannot produce a release build or signed
  launchable app on supported macOS;
- an update cannot preserve window/event-loop, input/IME, focus/actions, scroll/list,
  AccessKit, deterministic test, or packaging APIs without a newly accepted gate;
- macOS AX regresses the spike's workspace, conversation-history, text-input, action/value,
  or focus evidence. Production VoiceOver/Accessibility Inspector must additionally
  operate conversation, composer, board, and intervention controls with stable labels and
  focus order;
- initial project/conversation open requests all history, cached string payload exceeds
  the accepted 8 MiB harness bound, measured process memory violates a separately accepted
  product budget, or visible rendering grows with total history rather than the
  viewport/page;
- real supported-hardware presented-frame evidence misses the later accepted interaction
  budget under representative variable-height conversation and graph workloads;
- required GPUI/Zed code or dependencies introduce an unresolved license, provenance,
  security, or distribution constraint;
- GPUI or SwiftUI becomes a product-state/repository/rollout authority or creates a
  mutation path around `decodexd`;
- packaging requires an unsupported private entitlement, in-process Swift bridge, or
  signing/notarization arrangement incompatible with the existing release boundary.

Marked-text/IME composition remains absent: marked range is not exposed, marked selection
is ignored, and unmarking is a no-op. That gap does not invalidate XY-1263's required
minimum committed-text and accessibility path and does not block the later GPUI shell/cache
slice by itself. It **does** independently block acceptance of a production conversation
composer until a later interaction gate proves marked-text composition on supported macOS.

## Research continuity and gaps

The continuation scan covered all 43 tracked files under `openwiki/` and `spikes/`, plus
Git history, using `GPUI accessibility`, `accesskit activation`, and `XY-1263`. It found
the merged no-go artifact and its earlier history as the same decision under changed
runtime evidence. The unchanged rerun contradicted the old deterministic-no-content claim;
source inspection plus the public lifecycle candidate addressed that gap, but the later
38/40 result reopened the stability decision and the ad-hoc 40/40 lacked launch provenance.
The stale-base repo-local 40/40 closed the launch-provenance gap but could not support
landing. The fresh normalized current-main 40/40 closes both the base and outer-receipt
gaps and preserves the bound harness's nonactivating launch as the only failure in its
earlier 39/40 artifact.
External/private research remains unavailable, and the scan is substring-based, so
coverage remains candidate-search-only. Review and PR #1109 supersede the no-go outcome
only for the isolated pinned foundation. The accepted evidence does not authorize a
production shell or alter the remaining vNext product gates.
