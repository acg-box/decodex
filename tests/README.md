# Test maintenance

Test supported behavior, not the current spelling of its implementation.
`Makefile.toml` owns repository validation commands.

## Keep useful checks

- Exercise the public action and inspect its result. Use realistic state changes:
  reconnect, restart, rapid input, stale replies, and interrupted operations.
- Keep exact assertions for protocol values, persisted identities, permissions,
  duplicate dispatch prevention, and content preservation.
- Check layout relationships when they affect use: a control stays inside its
  container, an expansion appears below its trigger, or scrolling preserves the
  visible message. Do not fix incidental dimensions, colors, or typography in tests.
- Test motion math with supplied times. UI tests can wait for a transition to
  finish when they must check removal or hit testing. Intermediate wall-clock
  samples do not prove smoothness or frame rate.
- Use static checks for explicit ownership or safety boundaries. Prefer parsed
  manifests and configuration. Do not search for view modifiers, private helper
  names, exact source statements, or the names of other tests.
- Verify appearance and motion in the installed app. A passing unit test is not
  visual acceptance. Do not replace removed source snapshots with new ones.

## Cleanup coverage map (2026-09-28)

| Removed or reduced checks | Retained evidence |
| --- | --- |
| SwiftUI source snapshots for menu layout, icons, copy, glass, and animation | Menu presentation and real host-layout tests; installed-app visual review |
| Source snapshots for routing, login, and refresh calls | `AccountControlStoreTests`, `AccountControlCLIClientTests`, and `ResetCardStoreStartupRetryTests` exercise requests, readback, and stale replies |
| Source snapshots for bundle assembly and a fixed protocol minor | `scripts/macos/test_decodex_app_stage.sh`, the bundle verifier, and GPUI client lifecycle protocol tests |
| Theme opacity, spacing, duration, and particle-path constants | Visual review; motion reversal and reduced-motion behavior tests |
| Tab and disclosure intermediate frame samples | Close/reopen lifecycle, disclosure content order, refresh stability, and deterministic motion tests |
| Fake window-size assertions that only read constants | Panel shortcut and resize interaction tests |
| SQL text inventory and checks that a restart test exists | `scripts/vnext/local_database_gate.py` opens and inspects a real database; `database/tests/conversation_restart.rs` performs restart scenarios |
| Cargo configuration snapshots and duplicate retired-task checks | Stable compiler policy, lockfile use, formatter channel, and validation task dependencies |

The cleanup leaves protocol, recovery, database, and credential protections in
place. Static authority checks that remain are not runtime or security proofs.

## Broader review (2026-09-29)

The follow-up inspected Rust protocol and domain tests, database and transfer
coverage, CLI process tests, script gates, and repeated GPUI wire-test setup.
This is a targeted review of duplication and weak assertions, not a claim that
all tests in the repository have received a line-by-line audit.

- Removed the reset outcome test that asserted the length of its own four-item
  literal array. It could not detect a new enum variant.
- Removed a protocol-version constant copy. Combined version negotiation cases
  into current, different-major, and different-minor behavior checks.
- Removed a duplicate local configuration parse case. The retained configuration
  test already checks that local profile and also verifies its owner and policy.
- Removed static checks for retired protocol names, credential debug helper
  names, thread URL helper calls, transfer statements, and installer statements.
  Retained `wire.rs` decode tests, `credential_compare_and_swap_is_exact_and_debug_is_redacted`,
  provider thread ID/URL tests, `database/transfer/tests/transfer.rs`, and installer
  invocation tests execute those behaviors.
- Removed assertions that process acceptance tests have specific names or text.
  Retained the release fixture boundary checks and the executable acceptance tests.
- Shared temporary socket setup and welcome exchange across GPUI feature tests.
  Each feature still owns its scenario, expected request, and lost-reply assertions.
- Retained core routing, filesystem integrity, credential redaction, database
  restart, CLI process, and publisher dispatch protections. Similar vocabulary
  across these tests does not make their failure cases interchangeable.

The default nextest profile executes shared GPUI unit tests in the main app
binary. The screenshot tool and native-glass probe import those same modules,
so their duplicate test suites are filtered out. All targets still compile.
Use `--ignore-default-filter` only when diagnosing those diagnostic binaries.
The weather example has a separate parsing test and stays in the test set.
