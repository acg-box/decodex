# Effective voice before native call start

Classification: correctness for the existing Chief voice call. A new preference
picker remains a separate optional product surface in the manual catch-up.
Upstream source: `9c9451131fefe6c76e6dd97e8098300298442e64`, fixed at
`595cc91e8cbb1c2ca822d0311dcf12709410c582`.

## Behavior

Before each new V3 call, Chief reads the exact native thread and its effective
project configuration. The selected voice is sent with the existing realtime
start. A configuration failure occurs before a call receipt or native start is
created. No previous call's voice is cached as the next call's preference.

An unset preference uses the native V1/V3 catalog default, with Cove as the
upstream built-in fallback. The retained bridge admits the read-only voice catalog
method. Native servers without config/read retain the previous start behavior.
When a successful response contains an unknown future voice, the local override
is omitted. Other configuration errors propagate. This matches the fixed native
TUI realtime_settings and config_update owners.

The installed `0.155.0-alpha.16.4` schema confirms the optional voice in
ThreadRealtimeStartParams and the voice catalog. Its config parser rejects an
unknown configured voice enum. That is a failed read, not a successful future
voice observation; the call must not continue with a stale preference.

## Validation boundary

Adapter cases cover effective selection, native defaults, catalog fallback,
unknown successful values, unsupported config/read and malformed/failed reads,
with repeated reads across project identities. Coordinator cases verify the voice
sent at start and absence of call records or starts after a read failure.

An isolated installed-native process reads a trusted project override, then a
changed user default after the override is removed. It rejects invalid config and
reads the default catalog after the preference is removed. No inference, audio
subscription or production configuration is used by this fixture.

This batch does not add the optional voice picker or conditional preference write.
It does not establish microphone, WebRTC, audible voice or signed desktop acceptance.
Those items remain open in the full manual catch-up. Automation stays paused.

Validation results: installed-native and coordinator cases passed two tests;
full adapter suite passed189 with seven opt-in skips; runtime passed581 with44
opt-in skips. The native fixture ran separately from the default full suites.
Strict adapter and runtime Clippy passed for all targets and features.
