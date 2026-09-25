# Preserve an unreported account peak

Classification: correctness for the existing account profile, separate from the
optional full Analytics reports in the fixed manual catch-up.
Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.

The profile decoder previously replaced a missing peak_daily_tokens with the
maximum supplied daily bucket. A partial date window cannot establish a historical
peak. The fixed upstream backend profile tests and account_processor projection
preserve the optional field directly.

Decodex now preserves absent or null peaks as unknown and keeps an explicit zero
or reported value unchanged. Daily bucket sorting and bounds remain the existing
contract. No database or local protocol migration is needed.

The current AccountProfileRuntime passes the optional value to its existing
snapshot owner. A successful newer observation replaces the prior peak, including
with null; an older observation cannot overwrite the new result. The GPUI profile
facts only display Peak day when the optional value is present. A failed refresh
continues to expose the existing cached observation under the cached-profile state.
No production profile was fetched or modified for this fix.

Decoder tests distinguish absent, null, zero and reported values even with daily
buckets. The store test verifies successful refresh, cold reopen and stale-result
rejection across reported, missing and zero peaks. Runtime and wire projections
were inspected for direct optional-field preservation; existing profile tests are
part of validation. No new desktop layout or complete Analytics report is claimed.

## Remaining report scope

The current AccountApiRuntime is the shared authenticated backend read owner, and
AccountProfileRuntime owns persistence and cache publication. Full Analytics must
reuse the selected AccountService identity and bounded provider read path. Existing
quota windows and task estimates are not complete date-range reports or Top chats.
The native TUI report helpers do not by themselves supply a callable app-server
report API. Report routing, normalization, attribution, caching and UI remain
separate optional implementation and acceptance work.

Automation stays paused, including after the manual update.

Validation results: five selected adapter tests, the database refresh/reopen test
and ten selected runtime tests passed. Strict adapter and database Clippy passed
for all targets and features. The decoder now matches the preserved snapshot bytes.
