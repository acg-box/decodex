---
type: Guide
title: Test quality and coverage
description: Behavior-first test ownership, useful architecture checks, and the limits of cleanup and visual evidence.
tags: [testing, maintenance]
sources:
  - id: openwiki-source-95772fa38252ded838ec3a2f
    resource: repo://.config/nextest.toml
  - id: openwiki-source-30634b029f7a214a4cfa8636
    resource: repo://apps/decodex-gpui/Cargo.toml
  - id: openwiki-source-1b7747f8ae67c32e5670817d
    resource: repo://apps/decodex-gpui/src/agent_portfolio_tests.rs
  - id: openwiki-source-81428eaea331210561f63f66
    resource: repo://apps/decodex-gpui/src/bin/native_glass_probe.rs
  - id: openwiki-source-1bc5c8a6672bb09ef7b2d30f
    resource: repo://apps/decodex-gpui/src/bin/workbench_visual_capture.rs
  - id: openwiki-source-5c72291df76e3033136ad050
    resource: repo://apps/decodex-gpui/src/bin/workbench_visual_capture/send.rs
  - id: openwiki-source-37f9a508a2aebf9382d3b23f
    resource: repo://apps/decodex-gpui/src/main.rs
  - id: openwiki-source-f7ecbc6c02d8c1b7873858d6
    resource: repo://automations/upstream/checkpoint.test.mts
  - id: openwiki-source-7dcbb082d2502f1ec4386c39
    resource: repo://automations/upstream/prompts/maintainer.md
  - id: openwiki-source-3127fd4c7b2aefcc9cea7961
    resource: repo://automations/upstream/retained-contracts.json
  - id: openwiki-source-c740d34a4e6c4e581873e50e
    resource: repo://crates/decodex-account-login/src/lib.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-24139c353e393a9759b0267a
    resource: repo://crates/decodex-runtime/src/agent/tests/native_permissions.rs
  - id: openwiki-source-c8b1a2a9f2113ec43d4066da
    resource: repo://Makefile.toml
generated: { by: "codex", at: "2026-10-28T03:00:10.087Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-28T06:51:55.940Z
---

# Test quality and coverage

Keep a test when it can detect a meaningful change in supported behavior. Exact wire values, persisted identities, permissions and duplicate-effect prevention are contracts. A copied color, helper name, source statement or current catalog length usually is not.

## Choose the owner

| Contract | Retained evidence |
| --- | --- |
| Account-login callbacks and private state | Rust mock-issuer exchanges, callback bounds, private login-home cleanup and identity-drift tests |
| Client/service separation | Parsed Cargo dependencies and dedicated protocol surface checks |
| Provider refresh endpoint | Rust tests call the endpoint selector and loopback validator under the relevant build configuration |
| Recovery and persistence | SQLite restart, stale-owner, lost-reply and exact receipt scenarios |
| Desktop interaction | Input, focus, disclosure and lifecycle behavior tests; actual app review for visual quality |
| Packaging | Signed bundle inspection and native ABI checks |

A source scan can enforce a narrow explicit boundary, but it does not prove execution or security. Do not duplicate a behavioral test with assertions that its function name or implementation statement appears in a file. Keep license and provenance records intact when removing incidental source assertions.

Upstream implementation and test bodies are required source-review evidence, but generic Codex correctness remains upstream-owned. Daily maintenance executes affected Decodex consumers against the selected artifact. A targeted upstream test needs a concrete compatibility question or missing retained-fix proof. Workflow-only changes use the automation gate; do not repeat app or provider suites.

The checkpoint regressions protect observable state publication: a completed prefix updates cursor, certificate and remaining work together; stale input and unread evidence leave state unchanged; exact replay avoids duplicate counts; explicit archival preserves the original. They do not certify source-review truth or provide a second scheduler.

## Avoid duplicate execution

The main application owns the shared application tests. The visual-capture and native-glass diagnostic binaries disable their test and benchmark targets and exclude their bodies from test builds. The three capture-send tests use the same helper as the capture binary and run in the main application test target. Workspace test tasks select tests, examples and benchmarks without forcing the diagnostic binaries into empty test harnesses. Explicit `--all-targets` runs also produce no duplicate diagnostic tests. The diagnostic executables remain buildable, and independent targets such as the weather preview remain in the test set.

Process fixtures have a separate serialized test group. CLI shutdown fixtures reserve the available test threads so unrelated heavy work does not consume their bounded startup window. These scheduling rules protect test reliability; they are not performance acceptance.

The native-media group also runs one exchange at a time. Its packet-flow tests and the wall-clock collaboration-disclosure fixture reserve the runner's test threads while they check timing-sensitive behavior. The same reservation covers `parent_managed_composer_shares_lock_geometry_and_keeps_both_drafts` and `compaction_status_remains_visible_while_sending`, which click a moving control or sample a short exit frame. Keep their assertions, deadlines and default selection intact. This reservation does not establish frame rate or production latency.

## Share setup only when it helps

Small local fixtures can be clearer than a shared test framework. Share setup when its identity and lifecycle are truly the same. Keep different startup orders, native versus fabricated servers, and separate public consumers independent. Similar names do not make two failure scenarios duplicates.

Motion tests should use controlled times where possible. Fixed intermediate wall-clock samples do not establish smooth animation. Installed-app review is still needed for material, clipping, alignment and scrolling.

## Cleanup evidence

Nextest `LEAK` output means that captured output did not close within the runner's exit window; it does not by itself identify an application-owned survivor. The historical macOS Nextest 0.9.143 investigation reproduced sibling capture-pipe inheritance. The original test passed repeated isolated checks; the original marker had no pipe trace. Preserve those limits and diagnose each new marker on its own evidence. Do not change the timeout, skip tests or serialize the default suite merely to hide a marker. See the [validation guide](../operations/commands-and-validation.md#nextest-exit-markers) and its immutable historical diagnosis link.

Record the removed assertion, the contract it purported to protect, and the retained owner or reason it is no longer needed. Validate the affected owner before merging a coherent batch. A repository-wide search is candidate discovery, not proof that every scenario was reviewed. Do not use deleted line or test counts as a completion target.

Earlier cleanup reports remain in Git history. They do not establish exhaustive semantic coverage of the current repository. See [Commands and validation](../operations/commands-and-validation.md) and [Acceptance boundaries](upstream-acceptance-boundaries.md).

## Project and Work qualification

The separate retained Project/Work qualification records actual native and local test cohorts. Its original complete workspace run has 2918 executed, 2916 passed, two native audio ICE timeout failures and 1154 skipped. A passed appearance-fade test has a LEAK marker with unknown cause. Keep that original failed output. The separate media follow-up records a fresh current-main full run with all 2918 passed and no LEAK, plus 40 media executions on each of current main and the pre-change baseline, all passed without retries. The source and feature comparison and macOS firewall events support an external admission cause for the ICE stalls. Lock state was recovered retrospectively. The already-allowed package-only baseline passed while locked; after unlock, the workspace binary received a prompt and allow verdict. No new controlled lock or firewall-denial comparison was run. These later passes do not explain the earlier LEAK or replace its receipt. Use the stable contract catalog for exact identities, qualification limits and overlapping counts; use the [native media procedure](../integrations/subscription-voice.md#diagnose-native-media-ice-timeouts) for another ICE timeout.

The later native Project and nested Work follow-up retains a separate initial full run with 2918 passed, three failures and one passed-test LEAK. Two failures were the unchanged frame samples named above; the same test executable passed each sample five times in isolation. The other failure was an old native fixture missing `projectId`, now corrected to match the real response. The final full run passed 2921 tests with 1157 skipped and no LEAK. The first follow-up LEAK was in the native resume test and has no established cause. These later passes do not replace its receipt or prove a general scheduler defect. The index retains the original identities, the artifact digest, source comparisons and the ten isolated executions.

The later source-built lifecycle acceptance records another distinct result: its final full run passed 2924 tests with 1159 skipped and no LEAK. An earlier run passed all assertions but marked two tests as leaky; both unchanged tests passed in isolation. That clean result does not explain the earlier markers. The current source, native artifact, selectors and limits are under `retained_project_agent_work.live_acceptance` in the [stable contract catalog](../../automations/upstream/retained-contracts.json).

The control follow-up keeps its setup failures separate from product behavior. One native permission test first lacked its documented binary environment variable. It then exposed a stale fixture assumption: a per-thread automatic reviewer was expected to survive the persisted global Ask Me setting. The fixture now sets and checks global Automatic Review while retaining all profile, policy, directory and request-count assertions. The final selected cohort passed three native permission tests and one database hold test. Live UI and real-provider probes separately verified Pause/Resume and Full Access. The full workspace suite was not repeated for this test-only change.

The installed development follow-up adds two symptom regressions for independent-Agent conversation controls and input routing. Both failed before their fixes. Its complete UI package runs passed 1025 tests with 1031 skipped, but retained cleanup markers across three unchanged tests. Two isolated controls passed together and the third passed separately, without markers and with no unmatched selectors. The cause remains unknown. No assertions, deadlines or runner policy were weakened, and these package runs do not repeat the earlier full-workspace qualification. Exact identities and artifact limits are in `retained_project_agent_work.installed_acceptance`.
