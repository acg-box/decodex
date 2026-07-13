# XY-1263 GPUI feasibility evidence

Status: decision-ready **no-go** gate evidence; Manager acceptance and merge remain external.

As of: 2026-07-13. Repository base: `f9d6c4e70198e94e5b9461b8cac7518ae14d41ef`.
Authority: [vNext authority decision](../decisions/vnext-authority.md),
[vNext authority contract](../specs/vnext-authority.md), and
[vNext gate manifest](../specs/vnext-gates.md). The Linear design baseline is planning
provenance where it agrees with those sources.

## Verdict

Do **not** proceed with XY-1269 or later GPUI UI work. Zed commit
[`aeeacf5439b2d30d01e38d65d767e6f31b255ecc`](https://github.com/zed-industries/zed/commit/aeeacf5439b2d30d01e38d65d767e6f31b255ecc)
is the exact candidate tested, not an accepted downstream dependency. The isolated spike
passed macOS lifecycle, real Metal rendering, direct Unicode input/focus, bounded-list,
async, deterministic-test, bundle, and ad-hoc-signing probes. Its live macOS accessibility
bridge failed the minimum content check: recursive AX traversal exposed application and
menu elements but none of the declared workspace, history, or text-input nodes.

That failure triggers the repository gate's accessibility falsifier. The authoritative
outcome is a downstream freeze, not permission to choose another UI framework or restore
the legacy architecture. A future GPUI revision may replace this result only through a new
accepted feasibility run that passes content-level macOS accessibility and action/focus
probes.

## Provenance and dependency evidence

The selected revision was the official `zed-industries/zed` `main` HEAD returned by
`git ls-remote` at retrieval time. The commit is authored 2026-07-12T23:02:26Z. The Zed
application tag `v1.10.3` resolved to `0c54c414d522234de7298039708ffe85a116892a` and was
not selected: it is an application release tag, while this gate directly exercised the
newer current GPUI source. A moving branch, wildcard version, or unpinned Git dependency
is not accepted.

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
| `cargo +1.97.0 test --manifest-path spikes/gpui/Cargo.toml --release` | 3 passed, 0 failed, 1 measurement ignored. |
| `workspace_headless_frame_benchmark` (240 frames) | p50 276 µs, p95 332 µs, max 492 µs; 64 messages/2 MiB resident; 120 graph nodes. |
| `render_probe` real Metal (120 redraws) | p50 300 µs, p95 327 µs, max 533 µs; 2560×1600 capture with 488 unique RGBA colors. |
| `scripts/macos/stage_gpui_spike.sh` | Release `.app` staged; `plutil` clean; hardened runtime ad-hoc signature passes strict verification. |
| packaged launch + `CGWindowListCopyWindowInfo` | One on-screen `Decodex GPUI Feasibility` window, 1062×713 points. |
| `inspect_gpui_spike_accessibility.swift` | **Expected gate failure (exit 2):** trusted `AXApplication`, one reported window, 80 traversed application/menu elements, but all three required GPUI content labels absent, no text-input role, and no focused content label. |

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
pages, and an exact 8,388,608-byte cached string-payload peak. Five warmed-binary
executions took 73,021–74,773 µs internally (median 73,729 µs); `/usr/bin/time -l`
reported 16,728,064–16,760,832 bytes maximum RSS and 0.07 s wall time for every run. The
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
  committed Unicode text through `simulate_input`, left/backspace/end actions, and visible
  caret passed. Marked-text composition is not implemented or accepted: marked range is
  absent, marked selection is ignored, and unmarking is a no-op.
- Scrolling/virtualization: `uniform_list` owns scrolling and requested only the visible
  range; the paged provider enforced its independent 8 MiB string-payload bound.
- Graph: a real Metal frame contains a bounded 120-node canvas. This proves basic canvas
  composition, not production edge routing, animation, or graph-scale culling.
- Accessibility: the GPUI source declares stable IDs, headings, list items, graph nodes,
  group, and `Role::TextInput`, but the live packaged process does not expose those nodes
  through macOS AX. The recursive probe saw only `AXApplication`, menus, and menu items;
  it found no required GPUI labels, text field/area, or focused content label. This is a
  triggered gate falsifier, not deferred downstream work.
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

## Rejected candidate assumptions and update policy

No GPUI revision or API from this spike is authoritative for downstream implementation.
The following records the exact rejected candidate boundary so a replacement gate can be
compared without silently moving inputs:

1. Candidate Zed revision `aeeacf5439b2d30d01e38d65d767e6f31b255ecc`, Rust 1.97.0, macOS 14+, Metal,
   Xcode/Metal Toolchain, and the checked-in lockfile were the tested inputs.
2. `gpui_platform::application`, `Application::run`, `App::open_window`, `Entity`,
   `Render`, `FocusHandle`, `EntityInputHandler`, `uniform_list`, and GPUI test contexts
   worked as exercised here; declared AccessKit roles did not reach live macOS AX content.
3. Conversation data enters the UI through cursor/page adapters and bounded disposable
   caches. A project or conversation open never accepts an eager all-history API.
4. The graph and history renderer receive already bounded visible slices; GPUI is not a
   product-state or paging authority.
5. SwiftUI menubar and GPUI are protocol clients of `decodexd`, never peers sharing state.

Every replacement GPUI evaluation uses a new explicit revision, never a branch move or
wildcard. Update one commit at a time in an isolated branch, regenerate the lockfile, diff
GPUI/platform API and license metadata, run the complete XY-1263 command set, compare
history and frame evidence, and require live AX content roles/labels plus focus/action
behavior before review may accept the candidate. Keep this rejected commit recorded until
replacement evidence is accepted.

## Falsifiers and downstream stop conditions

XY-1269 and later UI work is frozen because the accessibility falsifier below occurred.
The remaining conditions continue to apply to any replacement evaluation:

- the exact revision/toolchain/Metal component cannot produce a release build or signed
  launchable app on supported macOS;
- an update cannot preserve window/event-loop, input/IME, focus/actions, scroll/list,
  AccessKit, deterministic test, or packaging APIs without a newly accepted gate;
- **Triggered at this pin:** macOS AX cannot expose the spike's declared workspace,
  conversation-history, and text-input content labels/roles or a focused content node.
  Production VoiceOver/Accessibility Inspector must additionally operate conversation,
  composer, board, and intervention controls with stable labels/focus order;
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

## Research continuity and gaps

The bounded continuity scan covered tracked `apps/` and `openwiki/` content plus Git
history at the base revision using queries `GPUI`, `virtualized list`, and `large history`.
It found the XY-1260 authority promotion and XY-1261 freeze provenance but no earlier GPUI
feasibility implementation. Coverage was candidate-only and file-limited (2,000 of 3,348
tracked files); unavailable/private prior research could still exist. This run adds the
first repository runtime/measurement evidence for this gate decision and does not supersede
the vNext authority contract.
