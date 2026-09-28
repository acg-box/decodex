> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile inherited usage research and delivered estimates

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
This reconciliation reads both complete inherited documents. It changes no
production code, account data, database schema or native configuration.

## Delivered task estimates

The inherited `work/task-usage-estimates.md` is covered by
[task usage estimate lifetime](task-usage-estimate-lifetime.md). The current
`crates/decodex-runtime/src/chief_usage_estimate.rs` compares process generation,
account, account revision, history revision, task and thread before and after the
native read. The GPUI owner in `apps/decodex-gpui/src/chief_usage_estimates.rs`
invalidates changed bindings and uses a panel epoch to reject late results.

The explicit collapsed query, observation time, separate provider groups, exact
integer micros and unknown-versus-zero distinction remain. Decodex does not adopt
the TUI minute poll or replace a reported zero with a previous positive estimate.
This is an explicit presentation difference, not missing native compatibility.
Live billing values and signed desktop account switching remain unqualified.

## Optional Analytics proposal

The full inherited [Analytics research](account-analytics.md) is preserved below
a current status notice. It originally reported that Analytics was unimplemented.
It contains report API research, not an inherited dashboard implementation.

| Research area | Current disposition |
| --- | --- |
| Typed report routes, date ranges, normalization, signed adjustments and attribution | Optional, unimplemented report consumer |
| Selected account/user identity, bounded refresh and report cache | Requirements if that optional consumer is selected; reuse AccountService |
| 7/30-day charts, grouping, filtering, independent loading and cancellation | Optional, unimplemented desktop surface |
| Business and consumer Top chats | Optional, unimplemented; one-task estimates cannot substitute for complete groups |
| Experimental Plan history | Optional, unimplemented; keep separate from current quota controls |
| Expanded Summary fields and 52-week activity | Optional, unimplemented; existing profile facts are not equivalent |
| Missing historical peak correction | Delivered core correctness fix in [account profile peak](account-profile-peak.md), PR1498 |
| TUI painting, keyboard aliases and terminal snapshots | Native terminal presentation; not a GPUI implementation dependency |

Current source inspection confirms that AccountApiRuntime reads the existing
`/wham/usage` and `/wham/profiles/me` endpoints. AccountProfileRuntime publishes the
existing bounded profile projection. The task estimate owner reads native
`account/usage/read` for one thread. These owners do not implement the inherited
report loader, Top chats query or dashboard. A Rust source search found no report
implementation; the account and UI owner inspection supports that result.

The old alpha.16 schema observation is historical. This reconciliation does not
claim fresh installed-binary Analytics support or run upstream report tests. It
does not turn research wording into a new feature obligation or count optional
Analytics as delivered. Preserve the research for the user's scope decision.

Close only the two inherited document rows after their content mapping. R09's
other consumers, shared source reviews and final desktop acceptance remain open.

## Validation

Three GPUI estimate tests and one runtime source-change regression pass on this
source. They cover exact micros, missing versus zero, task/source/thread changes
and late close/reopen results. The runtime case checks account, account revision,
history, process, thread and closed-source changes. No production source changed.
The original Analytics bytes are retained exactly after the status notice, and
both inherited document hashes match the preserved snapshot.
