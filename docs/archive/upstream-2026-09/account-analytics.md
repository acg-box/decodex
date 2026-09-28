> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Optional Analytics research: current disposition

Status on 2026-09-27: optional and not implemented. This is a preserved research
record, not an implementation queue or a claim of delivery. The user will decide
whether Decodex needs the full report surface during the subtraction review.
The fixed manual catch-up must distinguish this proposal from core compatibility
work. Automations remain paused.

The existing native per-task estimate and account profile are separate owners.
The profile peak correction is delivered; full date-range reports, Top chats,
Plan history and the expanded Summary are not. See
[the current scope mapping](usage-scope-reconciliation.md).

The text below is the complete inherited research, retained without changes.
Its installed-binary version, temporary test logs, intermediate review status and
imperative implementation wording are historical. They do not establish current
binary support, delivery or authorization to build a new dashboard. The upstream
commit references remain the original fixed-cutoff evidence.

---

# Account analytics catch-up

## Status and source

Account analytics is not implemented in Decodex. Existing account quota windows,
reset-credit inventory and task usage estimates do not cover this capability.

Reviewed upstream commits:
- `ab3b40c28b0e5eef750fe1f383bf58695031ced2`: typed report routes and models.
- `1fd53990043a11aa6aa9dae46ac7d1798ee91db5`: normalization and projection.

Both original patches and their relevant final source deltas were read. The
backend report client and models are unchanged at the fixed cutoff
`595cc91e8cbb1c2ca822d0311dcf12709410c582`. Account binding, report selection, token history, dashboard, Top chats and
Summary contracts through inventory 1346 are reviewed below. Terminal painting
and large snapshots were not exhaustively reviewed. Do not port the early
normalization unchanged.

## Contracts to preserve

Reports use inclusive UTC dates. Most reports send start_date/end_date/group_by;
consumer credit events have no date query and require local range filtering.
Enterprise token queries include both codex and work modes. Plugin/skill counts
request workspace_user=true and report-specific limits. Enterprise credits use
the requested breakdown. Preserve the api/codex versus wham path distinction.

Decode each endpoint into its matching model. Keep backend attribution, optional
amounts, signed credit adjustments, unknown string values, source dates, units
and freshness. A response decoding error must not include billing response text.

At cutoff, complete attribution across the requested period takes precedence over
legacy model/surface aggregates. Rows outside the requested period do not change
that selection. Task-start grouping includes all supplied trigger attribution;
the early user-only filter was removed. Legacy relative-model contributions below
1% group as Other only when complete attribution is absent. Merge duplicate dates
without losing each message record's unassigned remainder. Fill unreported dates
only for credit reports. Preserve missing versus explicit zero observations.
Freshness is not proof that the report is complete.

## Local integration boundary

`AccountApiRuntime` owns independent authenticated account reads, but currently
uses https://chatgpt.com/backend-api. The final upstream AnalyticsSession also
uses its configured account backend directly; model workspace routing is not a
precondition for report reads. See the corrected scope in
[workspace backend routing](workspace-backend-routing.md). Keep the selected account, workspace and credential
revision bound through request, refresh and result publication. Account changes
must invalidate pending results and displayed data.

The installed alpha.16 experimental ClientRequest schema has 164 methods, none
named analytics. Its account/usage/read accepts only an optional threadId and
returns summary/dailyUsageBuckets/threadUsage. account/workspaceMessages/read
accepts no parameters. These interfaces do not supply the full report/grouping/
date-range contracts above. Inspect their native owners and the later analytics
authentication commits before choosing the final read path; do not invent an RPC.

The existing AccountProfileController has selected-account/revision ownership and
loading state; use that pattern for the report UI. Reports need their own bounded
query/result state rather than global cached data or another task's native process.
Present supported reports and groupings for the server-reported plan, with clear
loading, unavailable, error and empty states. Do not claim full integration from
new DTOs or a passing schema check alone.

Acceptance remains open: report routing and decoding, signed/missing amounts,
attribution and denominator rules, exact account/workspace changes during reads,
refresh/restart/cross-client behavior, rendered report interaction and signed
application verification. No production report request or reset effect was made
while reviewing these commits.


## Token and authentication review (1331-1332)

Reviewed `7224096b852bd5b026543d87281efb950263f558` and
`db078158c302276b63f0d6ae3120433c6ca94a49`, including their tests and relevant
final source deltas. The final token normalizer is unchanged: grouped counts
precede per-model counts; use reported text totals for model grouping when
components are absent, while token-type grouping requires component data.
Explicit zero totals remain observable. Validate non-negative integral counts;
filter by requested date and model without mixing token and credit units.
Keep tiny signed credit adjustments visible and format integer millionths without
conversion to floating point.

Final AnalyticsSession fetches accounts/check once per new session and matches
the initial account ID. It does not select reports from token plan claims.
Require both ChatGPT account and user identity, disable HTTP redirects, recheck
identity before requests and when accepting results, and preserve the original
401 when bounded recovery fails. Failed lazy initialization can retry after login.
These native TUI helpers are not callable app-server analytics RPCs. Decodex's
AccountService remains its credential owner; do not copy the TUI auth.json loader
or introduce a second credential manager.

The final report loader captures an end date and keys cached payloads by report
and date range (consumer credit events have one unfiltered cache key). It checks
identity even on cache hits and caches only successfully normalized responses.
The later report-loader commit and its final tests still need complete review.


## Report loader review (1338)

Reviewed `1fc46a532b136feeff2be7960ea05e6c9f76167b`: final backend session,
report-loader, normalization and load-state source, original account-plan, identity,
routing, cache, retry and cancellation tests. Those upstream tests were not run.
Report availability uses HTTP results rather than invented plan restrictions;
unknown plans have no credit breakdowns. Replacing a pending load cancels it;
timeout, interruption, unavailable, empty and access errors remain distinct.
Cache reuse must recheck identity, and invalid normalized data must be evicted.

The native rate-limit reader also rechecks the current account and user after
backend reads before publishing account-bound calls to action. Do not infer that
the independent Decodex AccountApiRuntime has the same protection: inspect its
observation publisher and revision checks when integrating report reads.
The final source additions after this commit are plan history, display labels and
connection tests, which remain part of the later dashboard review.


## Dashboard interaction review (1339-1340)

Reviewed upstream `af3bc6f7961f6580b5d45ded38333a32fd994ff4` and
`ca53e19c757a5b0b4b1fa4627f0780c76062be63` by product contract and final owner.
Read original chart aggregation/callouts, complete initial view controller, menu
and application lifecycle adapters, final report lifecycle/grouping methods and
complete section/range owner. Read representative tests for full denominators,
signed values, independent failures, range/group preservation, close cancellation
and composer draft retention. Terminal painting primitives and snapshot fixtures
were not exhaustively reviewed or executed; GPUI does not consume that renderer.

The desktop integration must provide 7/30-day ranges, supported groupings, token
model filtering, daily details and refresh. Preserve each section's selection and
independent loading/error state. Account resolution precedes report requests;
refresh replaces the account-bound session and closes pending loads. Closing or
switching accounts cancels requests without changing the conversation draft.

Charts retain the source scale and authoritative daily total. Distinguish missing
days from reported zero, show signed credit bands, and keep the unplotted category
remainder without renormalization. Numeric labels must not be truncated into
misleading amounts. Selection must remain visible at narrow sizes. Terminal
colors, key bindings and cell painting are implementation details, not a desktop
API to transplant.

Final section selection includes later Summary, gated Plan history and Top chats.
Those contracts still need their own source review (1341-1346); the initial
five-section dashboard is not the final product scope. No desktop analytics view
has been implemented or visually accepted yet.


## Top chats and plan history review (1341-1342)

Reviewed the final business Top chats loader and backend batch contract from
`0e7ab7c1b5119ff0cc262b753e6b937b6c7ce13b`, plus original batch and loader tests.
The backend and final loader are unchanged at cutoff. List non-archived,
non-ephemeral root tasks active in the past 30 days; paginate, detect repeated
cursors and deduplicate IDs. Rank by lifetime credit estimates, not 30-day spend.
Use batches of 1-100 distinct IDs, reject unexpected/duplicate returned IDs even
on unavailable rows, and preserve partial unavailable estimates. Bounded 503
batch splitting is specific to this read API; it is not model-capacity recovery.
Hide local titles without a returned account estimate. Model, reasoning and speed
breakdowns, optional zero groups and supplied dollar estimates remain distinct.

Decodex already has exact task/account/process/revision checks around native
per-task usage estimates in chief_usage_estimate.rs. Reuse that ownership where
applicable; it is not a multi-task ranking UI or a batch analytics implementation.
The Top chats collection and view remain unimplemented.

Reviewed `0d0979f4576638c7b7bae7a9b2b4469c2f4664cc`: backend route/types/tests,
final period parser/state, original panel and period/gating tests. Plan history
requests days=7 and receives five-hour/weekly historical allowance periods.
404 means unavailable; 503 remains an error. Missing as-of or amounts must not
become current/zero. Preserve coverage, approximate boundaries, period completeness,
unknown dimensions and snapshot-scoped IDs; reset selections for a new snapshot.
Do not clamp valid percentages above 100 or replace historical denominators with
current account limits. The feature remains default-off at cutoff, exposed as
experimental by later1346. Enablement and consumer plan must both permit requests.
This history UI must not replace current quota or reset-credit controls.

Terminal table geometry and large snapshots were not exhaustively reviewed or
ported. Upstream tests were read, not run. Consumer Top chats, Summary and final
navigation commits still need review. No new local analytics implementation or
live billing acceptance is claimed.


## Consumer Top chats and Summary (1343-1344)

Reviewed `8f0d2459ac753e22cf0cf09999c3feac7b80b589` and
`9bd49c9dccc82491bb1ce5084f6fc6bba646d75b`: backend contracts and tests, final
loaders, display semantics, identity/refresh/cancellation tests and application
adapters. Backend and loader owners are unchanged at the cutoff, except the
later removal of the Summary grouping picker.

Consumer Top chats uses `/usage/thread_usage/query_v2`, with root creation times
and complete descendant groups, including archived descendants. Do not substitute
independent per-task estimates. Rank by reported current full five-hour/weekly
allowance percentages or exact decimal balance debits. Percentages can exceed
100; negative adjustments remain signed. Query only complete, disjoint groups;
reject cycles, unknown roots and duplicate returned IDs. Limit each request to
100 roots and 1,000 total IDs. Retain partial/unavailable status, missing amounts
and minimum batch freshness; hide unverified titles. The recent-root list can be
truncated and must disclose that coverage.

Summary independently loads `/profiles/me` and retains absent values. It includes
Fast Mode percentage, most-used reasoning effort, skill counts, total threads,
top plugin/skill invocations and freshness/partial-result metadata. Missing
invocations differ from an observed empty list; unknown invocation kinds do not
invalidate other statistics. Do not display raw backend stats_error text.
Summary percentages must be finite and between 0 and 100. Daily, weekly and
cumulative activity use a 52-week Sunday-aligned window; ignore future/invalid
buckets and accumulate duplicate dates without negative token reductions.

The native TUI reads its local authenticated account even with a remote app-server
connection. Decodex must use its selected AccountService identity instead. The
TUI no longer uses account/usage/read to populate Summary; this does not remove
that app-server method. Opening the view preserves queued transcript output and
composer state. Switching grouping does not reload the profile.

One existing local defect is fixed: the profile decoder no longer substitutes the
maximum of recent daily buckets for an unreported historical peak. Runtime and
wire projection preserve the optional value; a successful database observation
replaces an old value, and GPUI omits the missing fact. Cached observations remain
until refresh. Decoder tests cover absent, null, explicit zero and reported peaks;
all five profile-filtered tests and the decodex-codex strict lint pass in
`/tmp/decodex-1344-profile-tests.log` and `/tmp/decodex-1344-profile-lint.log`.
This fix does not implement the new Summary fields or full activity history.

## Final Analytics controls (1345-1346)

Reviewed product/controller changes and representative navigation/layout tests in
`de40696ec49815df76ec683f5ca6aea2b0f8760d` and
`b1f3c2f77e7cb802af0d8ef1c325cb6e9d39d8d9`. The Analytics directory and controller
are unchanged from 1346 to the cutoff. Grouping cycles through currently supported
choices, retaining other reports and Summary modes. Seven-day labels prioritize
the selected value without overlap or misleading truncation. Stable layout,
visible freshness, sufficient contrast and overflow-only scroll controls apply to
the desktop implementation; terminal cell geometry and h/l aliases do not.

Plan history is exposed as experimental and remains disabled by default. Explicit
key bindings override aliases, and aliases honor removed/remapped arrows; this is
a TUI interaction change, not a new app-server capability. Upstream tests were
read, not run. Full Analytics implementation and native/desktop acceptance remain
open; these source conclusions are not delivery evidence.
