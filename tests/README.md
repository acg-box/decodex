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
