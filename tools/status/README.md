# Status Tooling

This directory owns deterministic status artifacts that are not themselves
GitHub change bundles.

Current scripts:

- `build_reset_status.py`
- `validate_reset_status.py`

Contract ownership:

- output reset-status shape: `docs/spec/reset_status.md`

The current source is intentionally framed as a third-party community tracker.
CI may refresh this artifact on a schedule because no AI step is involved.
