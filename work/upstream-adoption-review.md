# Upstream adoption review

Original capability-table snapshot: Decodex `3f131d80b9e90d2badf2394249bbf3b0266f72d3`.
Current delivery snapshot: `9c22c8e7c2ca8e6bef992d07c45b693b7987be20` (PR1516).
The inherited-file byte audit remains at PR1507; it has not been rerun at this snapshot.
Fixed upstream range:
`a397079287e6638b39dda329835350d93222681f..595cc91e8cbb1c2ca822d0311dcf12709410c582`.

Status: the manual update and inherited-change reconciliation are not complete.
The 1,569-entry upstream inventory is a scan scope, not a count of adopted features.
This register combines early integration deliveries and later catch-up PRs. It does
not imply that every early PR came from one of those 1,569 commits.

See the [inherited-file audit](upstream-inherited-audit.md) for the 360 preserved
paths, exact content evidence and remaining core gaps.

Core means compatibility or correctness for an existing Decodex consumer. Optional
means a product behavior or control that the user can assess for removal. Mixed
rows need a finer split before removal. These labels are review classifications,
not instructions to delete code or weaken native enforcement.

## Current completion boundary

The source inventory covers 1,569 commits. It does not establish feature delivery.
The original acceptance ledger has twelve unequal groups. R01 and R02 have recorded
closure; R03 through R12 remain open. The table below retains those boundaries.
Later implementation does not close a group without its remaining acceptance.

| Group | Delivered or recorded evidence | Remaining exit condition |
| --- | --- | --- |
| R01 Permission settings | Native, journal, service and desktop flow; recorded group closure. | Shared signed application acceptance remains in R07/R12. |
| R02 Plugin, hook and app-link settings | Recorded group closure; distinct from embedded App UI. | Shared signed acceptance remains in R07/R12; widgets remain R05. |
| R03 Models, defaults and routing | Native defaults, explicit choices, ordinary recovery and profileless draft storage are merged. | Reconcile directory/service transitions, capacity/pre-profile input, cross-account fallback and uncertain-root materialization against actual consumers. Do not create a profile-switch feature from an old fixture checklist. |
| R04 Attachments, media and context | Resource add/list/remove, references and context presentation exist. | Verify final consumer behavior and installed file/image limitations; native internal storage is not a public byte-resolution API. |
| R05 Interactive MCP App UI | Integration metadata and settings exist. | Determine installed interaction support and complete the applicable widget consumer. Metadata alone does not close this group. |
| R06 Prompt editing and recap | Recap service/UI and opt-in automatic eligibility are implemented. Prompt editing has selection, journal, native mutation/recovery and public transport. | Complete canonical desktop draft editing and review; qualify remaining recap and editing end-to-end flows. See the feature notes below. |
| R07 Draft and signed desktop lifecycle | Draft persistence and source-bound recovery have targeted evidence. | Isolated signed-app blank-task/worktree, quit/menu/Dock/CmdQ, relaunch and export acceptance. |
| R08 Uncertain dispatch and closing | Known-unsent, refusal and no-replay recovery fixes are merged. | Reconcile general ambiguous replies and installed shutdown/unload races with current evidence. |
| R09 Other runtime consumers | Usage estimates, voice preferences, reasoning summaries and connector exposure have deliveries. | Close remaining Analytics, voice, provider/freeform, external-writer, accessibility and child/OS notice applicability. Do not rebuild existing usage owners. |
| R10 Native execution and security | Native CLI admission and account routing-cookie fixes are merged. | Map remaining Guardian/context, network/checkpoint and OS execution changes to native or local owners. Native ownership needs source evidence, not a duplicate implementation. |
| R11 Historical baseline and inherited changes | Preserved 360-entry snapshot, two stashes and original PR1378; partial adaptation evidence. | Complete pre-scan consumer dispositions and file-level reconciliation. Recheck version-specific limitations before presenting them as current. |
| R12 Final acceptance and handoff | Individual batches have merge and targeted validation evidence. | Close R01–R11, reconcile the final artifact, and deliver the complete core/optional/removal-dependency inventory. Keep maintenance paused. |

This is the remaining acceptance scope, not a new implementation backlog. Resolve
an item with implementation and evidence, or with a precise native-owned,
not-applicable or installed-version-limited disposition. An evidence gap alone
does not authorize an additional product feature.

The original PR1378 was still open at this refresh. Preservation does not prove
integration. The 268 non-identical inherited rows from the PR1507 audit are not
268 missing features. Do not remove the preserved snapshot or stashes on that basis.

## Capability inventory

| Capability | Classification | Current behavior | Merged PRs | Source owner |
|---|---|---|---|---|
| Native protocol, paged history and event projection | Core | Accept native metadata; preserve history, tool results, approvals, compaction and recovery under exact task identities. | [1337](https://github.com/acg-box/decodex/pull/1337), [1338](https://github.com/acg-box/decodex/pull/1338), [1342](https://github.com/acg-box/decodex/pull/1342), [1343](https://github.com/acg-box/decodex/pull/1343), [1370](https://github.com/acg-box/decodex/pull/1370), [1371](https://github.com/acg-box/decodex/pull/1371), [1375](https://github.com/acg-box/decodex/pull/1375), [1390](https://github.com/acg-box/decodex/pull/1390), [1403](https://github.com/acg-box/decodex/pull/1403), [1404](https://github.com/acg-box/decodex/pull/1404), [1405](https://github.com/acg-box/decodex/pull/1405), [1406](https://github.com/acg-box/decodex/pull/1406), [1409](https://github.com/acg-box/decodex/pull/1409), [1410](https://github.com/acg-box/decodex/pull/1410), [1415](https://github.com/acg-box/decodex/pull/1415), [1416](https://github.com/acg-box/decodex/pull/1416), [1417](https://github.com/acg-box/decodex/pull/1417), [1418](https://github.com/acg-box/decodex/pull/1418), [1419](https://github.com/acg-box/decodex/pull/1419), [1424](https://github.com/acg-box/decodex/pull/1424) | [crates/decodex-runtime/src/account_launch/chief_process.rs](../crates/decodex-runtime/src/account_launch/chief_process.rs) |
| Chief tool authority | Core | Keep delegated instructions, wakes and external results at native tool authority instead of recasting them as user instructions. | [1362](https://github.com/acg-box/decodex/pull/1362), [1365](https://github.com/acg-box/decodex/pull/1365) | [crates/decodex-runtime/src/chief.rs](../crates/decodex-runtime/src/chief.rs) |
| Questions and uncertain input | Core | Preserve exact replies, custom answers, explicit skips, drafts and uncertain delivery. Explicit Skip applies to asynchronous cards, not every provider request. | [1345](https://github.com/acg-box/decodex/pull/1345), [1346](https://github.com/acg-box/decodex/pull/1346), [1349](https://github.com/acg-box/decodex/pull/1349), [1351](https://github.com/acg-box/decodex/pull/1351), [1374](https://github.com/acg-box/decodex/pull/1374), [1391](https://github.com/acg-box/decodex/pull/1391), [1392](https://github.com/acg-box/decodex/pull/1392), [1394](https://github.com/acg-box/decodex/pull/1394), [1395](https://github.com/acg-box/decodex/pull/1395), [1396](https://github.com/acg-box/decodex/pull/1396), [1413](https://github.com/acg-box/decodex/pull/1413), [1414](https://github.com/acg-box/decodex/pull/1414) | [apps/decodex-gpui/src/chief_async_questions.rs](../apps/decodex-gpui/src/chief_async_questions.rs) |
| Models, tiers and defaults | Mixed | Native inheritance, catalog and dispatch correctness are core. New model controls are a product surface. Later ordinary-draft and recovery PRs preserve the selected settings through restart. | [1348](https://github.com/acg-box/decodex/pull/1348), [1361](https://github.com/acg-box/decodex/pull/1361), [1368](https://github.com/acg-box/decodex/pull/1368), [1389](https://github.com/acg-box/decodex/pull/1389), [1397](https://github.com/acg-box/decodex/pull/1397), [1399](https://github.com/acg-box/decodex/pull/1399), [1400](https://github.com/acg-box/decodex/pull/1400), [1401](https://github.com/acg-box/decodex/pull/1401), [1402](https://github.com/acg-box/decodex/pull/1402), [1407](https://github.com/acg-box/decodex/pull/1407), [1445](https://github.com/acg-box/decodex/pull/1445), [1446](https://github.com/acg-box/decodex/pull/1446), [1447](https://github.com/acg-box/decodex/pull/1447), [1448](https://github.com/acg-box/decodex/pull/1448), [1449](https://github.com/acg-box/decodex/pull/1449), [1450](https://github.com/acg-box/decodex/pull/1450), [1451](https://github.com/acg-box/decodex/pull/1451), [1452](https://github.com/acg-box/decodex/pull/1452), [1453](https://github.com/acg-box/decodex/pull/1453), [1454](https://github.com/acg-box/decodex/pull/1454), [1455](https://github.com/acg-box/decodex/pull/1455), [1456](https://github.com/acg-box/decodex/pull/1456), [1457](https://github.com/acg-box/decodex/pull/1457), [1458](https://github.com/acg-box/decodex/pull/1458), [1459](https://github.com/acg-box/decodex/pull/1459), [1460](https://github.com/acg-box/decodex/pull/1460), [1461](https://github.com/acg-box/decodex/pull/1461), [1462](https://github.com/acg-box/decodex/pull/1462), [1463](https://github.com/acg-box/decodex/pull/1463), [1464](https://github.com/acg-box/decodex/pull/1464), [1465](https://github.com/acg-box/decodex/pull/1465), [1466](https://github.com/acg-box/decodex/pull/1466), [1467](https://github.com/acg-box/decodex/pull/1467), [1468](https://github.com/acg-box/decodex/pull/1468), [1469](https://github.com/acg-box/decodex/pull/1469), [1470](https://github.com/acg-box/decodex/pull/1470), [1471](https://github.com/acg-box/decodex/pull/1471), [1472](https://github.com/acg-box/decodex/pull/1472), [1473](https://github.com/acg-box/decodex/pull/1473) | [crates/decodex-codex/src/app_server_client/model_defaults.rs](../crates/decodex-codex/src/app_server_client/model_defaults.rs) |
| Ordinary drafts without a profile | Core | Save and recover input before service setup; adopt it after the first profile is available. | [1474](https://github.com/acg-box/decodex/pull/1474) | [apps/decodex-gpui/src/ordinary_drafts.rs](../apps/decodex-gpui/src/ordinary_drafts.rs) |
| Account policy and routing | Core | Preserve account restrictions, refresh causes, plan names and the native workspace route used by quota activation. | [1355](https://github.com/acg-box/decodex/pull/1355), [1369](https://github.com/acg-box/decodex/pull/1369), [1388](https://github.com/acg-box/decodex/pull/1388), [1398](https://github.com/acg-box/decodex/pull/1398), [1411](https://github.com/acg-box/decodex/pull/1411), [1428](https://github.com/acg-box/decodex/pull/1428) | [crates/decodex-runtime/src/account_launch/activation_policy.rs](../crates/decodex-runtime/src/account_launch/activation_policy.rs) |
| Native usage observations | Core | Display native task estimates under the correct account/thread source; invalidate stale estimates. This is not billing analytics. | [1353](https://github.com/acg-box/decodex/pull/1353), [1477](https://github.com/acg-box/decodex/pull/1477) | [crates/decodex-runtime/src/chief_usage_estimate.rs](../crates/decodex-runtime/src/chief_usage_estimate.rs) |
| Existing integration sync | Core | Read independent plugin/MCP/App states, recover native OAuth, and explicitly refresh native App tools without retrying uncertain effects. | [1352](https://github.com/acg-box/decodex/pull/1352), [1482](https://github.com/acg-box/decodex/pull/1482) | [crates/decodex-codex/src/app_server_client/integrations.rs](../crates/decodex-codex/src/app_server_client/integrations.rs) |
| Voice capture correctness | Core | Bind callbacks and readiness to the active capture. Standard tests passed; opt-in loopback acceptance remains unresolved. | [1476](https://github.com/acg-box/decodex/pull/1476) | [work/voice-media-readiness.md](../work/voice-media-readiness.md) |
| Nonblocking provider-request timer | Optional | Still present: 60-second grace plus 60-second countdown, then one empty response when current/running request checks pass. Interaction stops the timer. This is separate from asynchronous-card Skip. | [1344](https://github.com/acg-box/decodex/pull/1344) | [apps/decodex-gpui/src/chief_requests.rs](../apps/decodex-gpui/src/chief_requests.rs) |
| Misalignment and Guardian review UI | Optional | Expose native review and explicit continuation decisions. Native security enforcement remains upstream-owned. | [1347](https://github.com/acg-box/decodex/pull/1347), [1354](https://github.com/acg-box/decodex/pull/1354), [1358](https://github.com/acg-box/decodex/pull/1358) | [apps/decodex-gpui/src/chief_guardian.rs](../apps/decodex-gpui/src/chief_guardian.rs) |
| Task resources and references | Optional | Add/list/remove native task attachments and insert explicit task references. | [1350](https://github.com/acg-box/decodex/pull/1350), [1373](https://github.com/acg-box/decodex/pull/1373) | [crates/decodex-runtime/src/chief_resources.rs](../crates/decodex-runtime/src/chief_resources.rs) |
| Archive and plugin installation controls | Optional | Restore archived native tasks and answer native installation suggestions under explicit user control. | [1359](https://github.com/acg-box/decodex/pull/1359), [1360](https://github.com/acg-box/decodex/pull/1360) | [crates/decodex-runtime/src/chief_host.rs](../crates/decodex-runtime/src/chief_host.rs) |
| Copy controls and rich Markdown | Optional | Copy responses/code and render formulas/Mermaid while retaining source copy. | [1363](https://github.com/acg-box/decodex/pull/1363), [1475](https://github.com/acg-box/decodex/pull/1475) | [work/rich-markdown-rendering.md](../work/rich-markdown-rendering.md) |
| Question notifications and native goal display | Optional | Notify on live questions and display native goals/accounting. These are additional product surfaces. | [1412](https://github.com/acg-box/decodex/pull/1412), [1425](https://github.com/acg-box/decodex/pull/1425), [1426](https://github.com/acg-box/decodex/pull/1426), [1427](https://github.com/acg-box/decodex/pull/1427) | [apps/decodex-gpui/src/chief_native_goal.rs](../apps/decodex-gpui/src/chief_native_goal.rs) |
| Reviewer, permission, plugin, hook and app controls | Optional | Expose native settings through reviewed commands and durable receipts. Core correctness inside these features must stay intact if the controls are retained. | [1420](https://github.com/acg-box/decodex/pull/1420), [1421](https://github.com/acg-box/decodex/pull/1421), [1422](https://github.com/acg-box/decodex/pull/1422), [1423](https://github.com/acg-box/decodex/pull/1423), [1429](https://github.com/acg-box/decodex/pull/1429), [1430](https://github.com/acg-box/decodex/pull/1430), [1431](https://github.com/acg-box/decodex/pull/1431), [1432](https://github.com/acg-box/decodex/pull/1432), [1433](https://github.com/acg-box/decodex/pull/1433), [1434](https://github.com/acg-box/decodex/pull/1434), [1435](https://github.com/acg-box/decodex/pull/1435), [1436](https://github.com/acg-box/decodex/pull/1436), [1437](https://github.com/acg-box/decodex/pull/1437), [1438](https://github.com/acg-box/decodex/pull/1438), [1439](https://github.com/acg-box/decodex/pull/1439), [1440](https://github.com/acg-box/decodex/pull/1440), [1441](https://github.com/acg-box/decodex/pull/1441), [1442](https://github.com/acg-box/decodex/pull/1442), [1443](https://github.com/acg-box/decodex/pull/1443), [1444](https://github.com/acg-box/decodex/pull/1444) | [crates/decodex-runtime/src/chief_config_settings.rs](../crates/decodex-runtime/src/chief_config_settings.rs) |
| Account recovery notices and notifications | Optional | Display recovery actions and send workspace-owner/usage-increase requests only after an explicit click. No automatic model fallback is delivered by this batch. | [1479](https://github.com/acg-box/decodex/pull/1479) | [work/account-recovery-notices.md](../work/account-recovery-notices.md) |
| Connector tool visibility controls | Optional | Edit per-connector omissions for initial tools, tool search and Code Mode. Share the existing config journal; native Codex owns actual filtering. | [1480](https://github.com/acg-box/decodex/pull/1480), [1481](https://github.com/acg-box/decodex/pull/1481) | [work/app-tool-exposure.md](../work/app-tool-exposure.md) |
| Unfinished output and live proposed plans | Mixed | Retain existing assistant text after termination/restart (core). Stream proposed plans with a distinct label (optional). Replace fallback only with exact complete native content. | [1486](https://github.com/acg-box/decodex/pull/1486) | [work/partial-output-retention.md](../work/partial-output-retention.md) |
| Reduced-motion transitions | Optional | Honor system reduced-motion and VoiceOver preferences. | [1478](https://github.com/acg-box/decodex/pull/1478) | [apps/decodex-gpui/src/ui_motion.rs](../apps/decodex-gpui/src/ui_motion.rs) |
| Configuration warnings and subagent activity | Core | Retain bounded native startup warnings and native subagent observations. Chief settings errors retain public native causes; ordinary warnings persist as Status history with exact history refresh. Partial-output retention remains a separate batch. | [1356](https://github.com/acg-box/decodex/pull/1356), [1357](https://github.com/acg-box/decodex/pull/1357), [1484](https://github.com/acg-box/decodex/pull/1484), [1485](https://github.com/acg-box/decodex/pull/1485) | [crates/decodex-runtime/src/native_config_warning.rs](../crates/decodex-runtime/src/native_config_warning.rs) |

## Deliveries after the original table

All 20 PRs below were read from GitHub as merged. Their merge commits are
ancestors of the PR1507 table baseline. Each row describes delivered behavior; test
and live-acceptance limits remain in its linked feature note or PR. These are
additional PRs, not a count of additional upstream commits or independent features.

| Capability | Classification | Result and subtraction boundary | Merged PRs |
| --- | --- | --- | --- |
| Native positive refusal and capacity cancellation | Core | Preserve unsent Chief/ordinary input and exact refusal receipts without replay. Keep these correctness rules for existing input consumers. | [1488](https://github.com/acg-box/decodex/pull/1488), [1489](https://github.com/acg-box/decodex/pull/1489) |
| Complete approvals and live file review | Core | Retain large approval payloads, read complete source-bound pages and preserve explicit decisions and file evidence. | [1490](https://github.com/acg-box/decodex/pull/1490), [1491](https://github.com/acg-box/decodex/pull/1491), [1492](https://github.com/acg-box/decodex/pull/1492), [1493](https://github.com/acg-box/decodex/pull/1493) |
| Model access program display | Optional | Show observed catalog metadata; this grants no access and selects no program. | [1494](https://github.com/acg-box/decodex/pull/1494) |
| Effective voice configuration | Core | Apply the effective native configuration before an existing voice call. Keep separate from the new preference picker. | [1495](https://github.com/acg-box/decodex/pull/1495) |
| Voice preference picker | Optional | Choose native preferences for future calls through the settings surface. | [1496](https://github.com/acg-box/decodex/pull/1496) |
| Public reasoning summary display | Optional | Render native public summaries with provenance; do not enable or expose raw reasoning. | [1497](https://github.com/acg-box/decodex/pull/1497) |
| Missing profile statistics | Core | Preserve unknown historical peaks instead of presenting them as observed values. Full Analytics remains open. | [1498](https://github.com/acg-box/decodex/pull/1498) |
| Dependency and CLI compatibility | Core | Repair inherited advisories and CLI approval pagination compatibility. | [1499](https://github.com/acg-box/decodex/pull/1499) |
| Account routing affinity | Core | Retain the account HTTP routing cookie under the existing consumer. | [1500](https://github.com/acg-box/decodex/pull/1500) |
| Signed native CLI admission | Core | Preserve the installed CLI bundle context and verify its actual main executable. This repair supports all native execution, not only recaps. | [1504](https://github.com/acg-box/decodex/pull/1504) |
| Manual recap and saved voice input | Optional | Add isolated generation, desktop controls, source invalidation and stored spoken context. Review its temporary request helper together with the recap consumer; automatic eligibility was subsequently delivered in PR1510; signed/live acceptance remains open. | [1501](https://github.com/acg-box/decodex/pull/1501), [1502](https://github.com/acg-box/decodex/pull/1502), [1503](https://github.com/acg-box/decodex/pull/1503), [1505](https://github.com/acg-box/decodex/pull/1505), [1506](https://github.com/acg-box/decodex/pull/1506), [1507](https://github.com/acg-box/decodex/pull/1507) |

## Later optional display

[Model access metadata](model-access-programs.md) adds an optional Chief model
notice after this register's historical baseline. The shared ordinary/Chief
catalog carries the observed programs; only Chief gains the detail display.
Removing that notice and informational projection does not require removing the
native model catalog or account-source checks. The model-access note records
current validation and limits. Its presence does not close the other optional
analytics, voice, widget, prompt-editing or recap scopes below.

[Voice preferences](voice-settings.md) adds an optional explicit picker after this
baseline. Its conditional save path is separate from the existing-call effective
voice fix in [PR1495](https://github.com/acg-box/decodex/pull/1495). The note records
source checks, readback and the outstanding signed audio acceptance.

[Public reasoning summaries](public-reasoning-summaries.md) adds a separate optional
live/history display after this baseline. It does not enable a new summary mode
or expose raw reasoning. Native and rendered-test evidence and remaining limits
are recorded in the feature note.

[Account peak correction](account-profile-peak.md) preserves missing versus reported
profile statistics after this baseline. This fixes the existing profile; it does
not deliver full Analytics reports or Top chats.

[Dependency repair](dependency-security-repair.md) resolves three inherited RustSec
findings in [PR1499](https://github.com/acg-box/decodex/pull/1499). It also restores
CLI compatibility with the delivered approval pagination. These are core fixes.

[Account routing affinity](account-routing-cookie.md) restores the upstream
infrastructure routing cookie for the existing account HTTP consumer. It adds no
account authentication cookie storage or optional product control.

## Evidence boundaries

The original table cited 128 distinct PRs, checked at its original snapshot. The refresh separately checks the 20 later PRs above; it does not reclassify nearby unrelated work as scan adoption. Current source owners were inspected or located in this checkout. This confirms merge and source presence, not full live acceptance or release installation. No new behavioral test was run for this documentation reconciliation.

- The old nonblocking request timer remains active in `chief_requests.rs::tick_question_timeout`. Do not infer its removal from the separate asynchronous-question changes.
- Native login-policy qualification in PR1452 does not prove that Decodex's independent browser/device-code account-enrollment UI enforces that policy. Its applicability and authority still require a decision.
- Flex preservation in Decodex and actual native provider routing are different claims. Historical alpha.16.3 failures do not prove the installed alpha.16.4 outcome; retain separate provider/version evidence.
- Workspace routing applies to native model requests and the adapted quota activation path. It is not authority to redirect every account/profile/reset API to a model backend origin.
- Native execution, provider transport, tool filtering, enterprise registration and security remain Codex responsibilities. Source review does not justify a second local implementation.

## Not counted as new scan adoption

Chief architecture/voice baseline, Dock/Glass polish, Reset Cards, weekly quota activation, packaging, lint repairs, OpenWiki updates and automation scheduling changed in nearby PRs. Co-occurring merge dates do not establish that the upstream scan adopted them. Retain those product histories separately; do not remove them as scan cleanup without their own scope decision.

## Preserved original work

The PR1507-baseline audit matched all 357 preserved file contents to the takeover
manifest, with no mismatch, and retained the three deletion records. This refresh
rechecked both stash commit objects and original PR1378, which is still open.
These are preservation facts, not claims that every inherited change reached main.

## Still open

- Complete the disposition of the recovered 360 paths, both preserved stashes and original PR1378. A preserved file is not necessarily merged; a shared file can contain both delivered and outstanding changes.
- Resolve remaining inherited analytics, native diagnostics, model/access policy, media and shared-file differences against current owners. Delivered voice preferences and catalog access display do not close those separate differences.
- Complete applicable MCP App UI, earlier-prompt editing and recap work in the authorized fixed pass, with separate optional-feature labels.
- Complete signed desktop, live voice/connector and remaining native-owner acceptance or record precise supported limitations.
- Reconcile the historical baseline and full fixed-cutoff evidence before claiming completion.

The upstream maintainer remains PAUSED. Completion does not authorize enabling it.

## Deliveries after PR1507

GitHub reports all PRs below as merged, and each merge commit is an ancestor of
this snapshot. PR1508 refreshed the adoption and inherited-file records. The
following implementation batches extend that record; they are not independent
feature counts.

| Capability | Classification | Current result and subtraction boundary | Merged PRs |
| --- | --- | --- | --- |
| Automatic recap | Optional | Persisted preference and selected-task desktop lifecycle; off by default. Installed-native synthetic-provider and rendered evidence exist. Signed desktop, live voice and combined lost-reply acceptance remain open. Removing recap must retain the output-observation fix below. | [1509](https://github.com/acg-box/decodex/pull/1509), [1510](https://github.com/acg-box/decodex/pull/1510), [1511](https://github.com/acg-box/decodex/pull/1511) |
| Live output I/O | Core | Move the existing long-lived output connection off the shared desktop executor. Preserve this fix if recap is removed. | [1511](https://github.com/acg-box/decodex/pull/1511) |
| Earlier-prompt editing | Optional, incomplete | Canonical native selection, durable receipt, one-shot revert/recovery and public paged transport are merged. GPUI review, canonical draft editing/storage and end-user acceptance remain open. Applied native history is not desktop draft restoration. | [1512](https://github.com/acg-box/decodex/pull/1512), [1514](https://github.com/acg-box/decodex/pull/1514), [1515](https://github.com/acg-box/decodex/pull/1515), [1516](https://github.com/acg-box/decodex/pull/1516) |
| Native revert and capacity retry | Core | Cancel unclaimed continuation after an owned native revert; preserve claimed attempts and receipts. Retain this fix if the editor is removed. | [1513](https://github.com/acg-box/decodex/pull/1513) |

See [recap integration](task-recaps.md), [output observation](chief-output-observation.md)
and [prompt editing](prompt-editing.md) for contracts and validation limits.
Prompt-edit removal must account for outstanding durable receipts and the schema43
minimum-reader contract; hiding its UI is not authority to discard recovery state.
The [native CLI bundle admission](codex-cli-bundle-compatibility.md) repair remains
core for all native execution, even if both optional features are removed.

This refresh checks documentation consistency and merged ancestry. It adds no
application behavior and does not rerun the earlier runtime or desktop tests.
At readback, PR1516 Dependency Review and JavaScript/Python CodeQL passed; Rust
CodeQL was still running. Merge state is not evidence that every check finished.

## Maintenance policy

Complete this fixed manual pass before the user's subtraction review. Future
necessary compatibility and correctness changes must identify an existing Decodex
consumer. Optional product additions require notification and user selection.
The scheduled upstream maintainer remains PAUSED, including after manual completion.
The old instruction to restore daily UTC20:05 execution is superseded. Automatic
recaps are a separate, opt-in product preference; they do not enable maintenance.
