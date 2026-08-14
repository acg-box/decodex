# XY-1368 retained-title freeze

XY-1364 remains the historical V14-V21 acceptance. XY-1368 adds V22 to that accepted core.
The V22 acceptance does not reopen the landed V14-V21 history.

## Acceptance receipt

The canonical semantic boundary emits a
`decodex/server-store-retained-title-acceptance/1` receipt. Cleanup emits the associated
`decodex/server-store-retained-title-stage-report/1` evidence. The receipt binds the base commit and
the exact staged source tree at start and completion.

The accepted evidence must contain these facts:

- The migration ledger contains exactly V1 through V22.
- V22 is `retained_title_experiment_bridge`.
- Runtime execute and type grants match the exact V22 inventory.
- Schema and configured-authority digests match the checked-in constants.
- The creation fence and title fence each permit one effect.
- The start result binds to the exact request and returned thread ID.
- `thread/read` uses the exact returned thread ID.
- A positive retained-title attestation exists before the observation.
- The attested observation permits the V17 eligibility transition.
- Production code cannot reach the manual retained-title runner.

The receipt records the pinned `codex-cli 0.145.0-alpha.18` protocol facts.
The start result name is nullable. `thread/name/set` is a separate mutation.

## Deferred matrix

| Work | Owner | XY-1368 disposition |
| --- | --- | --- |
| Live thread creation and Desktop discovery | XY-1363 | Deferred |
| Aggregate validation and production enablement | XY-1304 | Deferred |
| Trusted `decodex/local-full-check` publication | XY-1304 | Deferred |
| Final landing | XY-1304 | Deferred |

The production route remains disabled. XY-1368 does not change account routing or plugins.
