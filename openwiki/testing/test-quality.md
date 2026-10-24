---
type: Guide
title: Test quality and coverage
description: Behavior-first test ownership, useful architecture checks, and the limits of cleanup and visual evidence.
tags: [testing, maintenance]
sources:
  - id: openwiki-source-95772fa38252ded838ec3a2f
    resource: repo://.config/nextest.toml
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-c740d34a4e6c4e581873e50e
    resource: repo://crates/decodex-account-login/src/lib.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
generated: { by: "codex", at: "2026-10-24T21:11:31.338Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-24T21:19:56.719Z
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

## Avoid duplicate execution

The default nextest filter excludes shared application tests from the workbench visual-capture binary and excludes the native-glass diagnostic binary. The main application owns those shared tests. The visual-capture binary's own `capture_send_tests::` remain selected. All targets still compile. A target with independent behavior tests, such as the weather preview, remains in the test set.

Process fixtures have a separate serialized test group. CLI shutdown fixtures reserve the available test threads so unrelated heavy work does not consume their bounded startup window. These scheduling rules protect test reliability; they are not performance acceptance.

The native-media group also runs one exchange at a time. Its packet-flow tests and the wall-clock collaboration-disclosure fixture reserve the runner's test threads while they check timing-sensitive behavior. This reservation does not establish frame rate or production latency.

## Share setup only when it helps

Small local fixtures can be clearer than a shared test framework. Share setup when its identity and lifecycle are truly the same. Keep different startup orders, native versus fabricated servers, and separate public consumers independent. Similar names do not make two failure scenarios duplicates.

Motion tests should use controlled times where possible. Fixed intermediate wall-clock samples do not establish smooth animation. Installed-app review is still needed for material, clipping, alignment and scrolling.

## Cleanup evidence

Nextest `LEAK` output means that captured output did not close within the runner's exit window; it does not by itself identify an application-owned survivor. The historical macOS Nextest 0.9.143 investigation reproduced sibling capture-pipe inheritance. The original test passed repeated isolated checks; the original marker had no pipe trace. The earlier component qualification records a separate marker and passing isolated rerun, without claiming the same cause. Preserve those limits and diagnose each new marker on its own evidence. Do not change the timeout, skip tests or serialize the default suite merely to hide a marker. See the [validation guide](../operations/commands-and-validation.md#nextest-exit-markers) and its immutable historical diagnosis link.

Record the removed assertion, the contract it purported to protect, and the retained owner or reason it is no longer needed. Validate the affected owner before merging a coherent batch. A repository-wide search is candidate discovery, not proof that every scenario was reviewed. Do not use deleted line or test counts as a completion target.

Earlier cleanup reports remain in Git history. They do not establish exhaustive semantic coverage of the current repository. See [Commands and validation](../operations/commands-and-validation.md) and [Acceptance boundaries](upstream-acceptance-boundaries.md).

## Project and Work qualification

The separate retained Project/Work qualification records actual native and local test cohorts. Its complete workspace run has 2918 executed, 2916 passed, two native audio ICE timeout failures and 1154 skipped. A passed appearance-fade test has a LEAK marker with unknown cause. One timeout reproduced on untouched main under workspace features; that does not prove both failures are baseline defects. Passing focused Work/Guardian tests does not make the complete suite green. Keep the original failed output in native receipts and use the retained index for exact identities and overlapping counts.
