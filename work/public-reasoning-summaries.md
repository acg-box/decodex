# Optional public reasoning summaries

Classification: optional presentation in the fixed manual catch-up.
Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
The native protocol defines ReasoningSummaryTextDeltaNotification and reasoning
items with a summary array. The fixed TUI reasoning-default owner uses none;
this adaptation does not force the earlier detailed mode or write user settings.

## Live and stored behavior

Chief reads only summaryTextDelta and completed summary parts. It never stores
raw reasoning textDelta or reasoning content as summary text. Schema41 extends
the existing live-output table with ordered summary parts. Completion replaces
partial parts, and late deltas cannot append to a completed item. The existing
process, work and active-turn owner checks remain in force.

The preview retains at most32 items,32 parts and64KiB per item. Unicode boundaries
and explicit truncation survive reopen. Even an out-of-range first part records
truncation, which a later complete item can clear. Successful updates notify the
existing output revision channel. Summary observations do not enqueue agent work
or become unfinished assistant-answer receipts.

Native voice-delegation markers preserve the set of summary items seen before the
handoff. Later voice summaries are excluded. Local provenance records are filtered before saved-transcript pagination;
they do not add internal status messages. This is display provenance, not a new
permission or execution authority. Cold history without a local marker reads all
bounded native item pages for the exact turn before projecting summaries. It does
not infer provenance from only the visible timeline page.

## Presentation

Protocol2.83 distinguishes ReasoningSummary from Plan and AgentMessage. Both live
and native history use a Reasoning summary label. Exact native turn/item identity
prevents duplicate rows. While the turn is active, the native row can use its live
summary; a history item's presence alone does not prove stream completion.
Raw reasoning content remains outside both the timeline and activity detail.

## Evidence and limits

The schema40-to41 upgrade preserves existing live plans and the applied migration
ledger. Database tests cover ordered deltas, reopen, truncation, completion,
late deltas, wrong owners, voice origins and revision notification. Runtime tests
cover summary-only projection, voice filtering and a handoff on an earlier native
item page. The rendered GPUI test checks exact-item deduplication for plan and
summary rows.

An isolated Codex0.155.0-alpha.16.4 process receives one synthetic Responses reply
with public summary and raw-reasoning fixture fields. Chief records the public
summary before turn completion. The retained timeline projects it without the raw
field and returns the same page after a cold restart, without another inference.
This verifies native transport and recovery under a loopback provider. It does not
prove production provider summaries, maximum-size history latency or signed
desktop visual acceptance.

For subtraction, remove the optional summary projection, live observation and
presentation together. Keep native history, voice source identity and other
live-output behavior. Preserve old migration entries and stored receipts; any
storage retirement needs a forward migration. The full catch-up remains open.
Automation stays paused after delivery.

Validation results: full database131, adapter191/eight opt-in, protocol136 plus six
integration, runtime588/46 opt-in, and GPUI468/five opt-in passed. The final
provenance transcript filter and test refactor passed the two focused database
checks. The installed-native scenario passed separately with the reasoning
regressions. Signed desktop acceptance remains separate from rendered test windows.
Strict database, adapter, protocol, runtime and GPUI Clippy passed. After simplifying
the renderer selector, all three native timeline rendering regressions passed again.
