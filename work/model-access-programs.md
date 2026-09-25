# Model access metadata

Classification: optional product display for the manual fixed-cutoff pass.
Upstream commit: `e3a52b87b28760413eafa340e2ab23d653f0bbe7`.
Fixed cutoff: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.

The native model catalog exposes caller-specific access programs. Its protocol
owner is `app-server-protocol/src/protocol/v2/model.rs`; native projection and
remote-catalog tests preserve `availableAccessPrograms` from model discovery.
The schema generated from installed Codex `0.155.0-alpha.16.4` confirms this field
and the standard, daybreakBlue and daybreakRed values.

## Delivered behavior

Protocol2.81 carries the known advertised names through the shared ordinary and
Chief model projection. Missing or null metadata stays unknown; an explicit empty
list stays empty. Unknown future strings are ignored without rejecting the model;
duplicate known names are collapsed. Malformed non-string entries invalidate the
catalog under the existing projection contract.

The Chief model detail panel shows the current observation. Refresh can change or
remove it. Existing account, directory and process source ownership applies to the
whole catalog. Ordinary model reads carry the same metadata; this batch does not
add a second detail panel to the ordinary composer.

This display never selects a program, changes the model, stores an authorization
grant or interprets access as quota. Native automatic program selection and
inference authorization remain upstream. There is no Daybreak preference control.

## Validation and subtraction boundary

The installed-native fixture uses a synthetic account and loopback model endpoint.
Across cold starts with a stable ETag, it observes populated, empty and missing
metadata and checks the native cache. Unknown backend names are filtered by the
native owner. This proves catalog transport and refresh, not enterprise access
or inference authorization.

Projection tests cover metadata changes and wire serialization. Existing ordinary
catalog tests cover the shared reader. A rendered GPUI regression changes the
observation through populated, empty and absent states while retaining the selected
model. Signed desktop visual acceptance remains open.

For subtraction, remove the display and its informational DTO/projection field as
one optional unit. Keep the existing model catalog, account-source checks, native
routing and permission enforcement. Automation remains paused after delivery.

Validation results: the isolated installed-native test passed; runtime catalog
selections passed eight plus three tests; protocol passed136 unit and six
integration tests; GPUI passed465 with five opt-in skips. Strict protocol, runtime
and GPUI Clippy passed for all targets and features. No signed application or
real enterprise account authorization test was performed.
