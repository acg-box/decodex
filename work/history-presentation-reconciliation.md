# History presentation reconciliation

## Scope and restored behavior

Compare the complete inherited `chief_timeline.rs` and
`chief_timeline_receipts.rs` files with their current owners. The preserved
snapshot starts at `2ffa385c3b49efe6a4109de0fd7353fb64abd2c5`.
This review closes these two files only. The larger timeline renderer,
workspace, Chief surface and signed desktop acceptance remain open.

A latest-history request lost its bottom-scroll intent when a summary arrived.
The summary callback cleared the viewport before it consumed that intent.
Restore the inherited summary helper: consume the request, cancel the old
anchor, accept the summary, then apply the requested bottom scroll. A wheel
gesture that cancels the request before arrival still takes precedence.

## Complete file mapping

| Inherited behavior or difference | Current owner and disposition |
| --- | --- |
| Native source, request epoch and page validation | Retained in the timeline loader and state. Summary acceptance requires an initial request and the matching work and thread. Invalid summary results use existing failure handling. |
| Summary refresh and latest scroll | Restored in `refresh_native_summary`; the current anchor cancellation occurs after the latest request is consumed. |
| Prompt handback | Additive `restore_prompt_presentation` requires selected work, matching thread, complete question recovery and a valid full native page before it replaces history. It clears old reads, output and navigation through current owners. |
| Native history prefetch | Additive entry uses existing cursor, retry, pending-read and viewport checks. A bound native view does not fall through to local automatic paging. |
| App recovery and streaming output | Additive App state clears with its page or binding. Streaming fallback requires the active turn and absence of the exact native item. |
| Summary state and full-page replacement | Retained. Summary items do not acquire native positions, cursors or voice boundaries. Full pages clear summary state. |
| Saved receipt ordering and duplicate IDs | The current BTreeMap extension retains the old insertion order semantics: current rows replace older rows with the same ID. |
| Partial output replacement | Current exact thread, turn, item and kind match requires complete, nonempty native content. A truncated row, another source or a terminal boundary alone cannot remove the saved fallback. |
| Uncertain input, checklist and recovery records | Existing private delivery evidence and current controls remain separate from native conversation rows. No text-based deduplication is introduced. |
| Labels | Unsent input is explicit. The plan label uses “In progress” with the same live-plan owner. |
| Inherited tests and DTO changes | Retain the original cases with current native source, turn and weather fields. Restore the unfinished-answer and unfinished-plan case in both saved and native views; use actual copy controls and assert exact original text. The obsolete receipt voice-session field is not restored as a second voice owner. |

## Validation

The rendered latest-history regression fails before the fix: summary arrival
leaves 2221 pixels below the viewport. After the fix, the same test covers full
pages and summaries, an existing or absent scroll handle, and a later wheel
gesture. All six cases pass.

All 38 native-timeline tests pass, including source changes, summary copying,
partial-output replacement, prompt handback, App recovery and pagination.
The restored unfinished-output case copies the exact incomplete LaTeX source
from both views. Its first adaptation used an obsolete row selector; that
fixture failure was not evidence of lost output.

Stable Rust strict Clippy passes for all GPUI features and targets. Local logs:
`/tmp/decodex-summary-scroll-before.log`,
`/tmp/decodex-summary-scroll-final.log`, and
`/tmp/decodex-summary-scroll-clippy.log`.

## Product decision boundary

Correct source handling, preserved output and requested navigation are core
correctness for existing consumers. Summary recovery and the saved-record view
are optional presentation choices. A later removal decision must retain
readable delivery evidence and exact-source rules in the selected consumer.
This batch adds no native execution owner or automatic maintenance. These
rendered tests do not establish signed desktop acceptance or live-provider
recovery.
