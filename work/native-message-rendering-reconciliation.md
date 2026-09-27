# Native message rendering reconciliation

## Scope

Compare the complete inherited
`apps/decodex-gpui/src/chief_timeline_render.rs` with its current owner.
The preserved snapshot starts at
`2ffa385c3b49efe6a4109de0fd7353fb64abd2c5`.
This review closes only that file. It does not close the shared Chief surface,
output-stream owner, whole history workflow or signed desktop acceptance.

## Restored correctness

The current renderer adds saved message metadata to native messages. Its lookup
matched only turn and text and excluded partial output. If a user and an assistant
had the same text in one turn, the assistant could inherit the user's kind.
That changes its layout and removes the assistant copy control. Require a
compatible role before copying metadata: native user messages accept saved user
or manager-instruction rows; native assistant messages accept saved assistant
rows. Keep exact turn and text checks and preserve the existing metadata owner.
If no compatible row exists, use the existing native message projection.

The extracted streaming branch also omitted the inherited response copy control.
Keep the current text-reveal renderer and restore copying of the exact in-progress
source. Completed messages still use the existing shared history renderer.

## Complete mapping

| Inherited area or difference | Current owner and disposition |
| --- | --- |
| Summary rows | Retain distinct summary identity, native author label, original markdown, attachment previews, omission notice and assistant copy action. |
| Native row identity | Retain work, thread and native row key. Current anchor and scroll wrappers remain the only navigation owners. |
| Live output | Retain exact turn/item lookup and terminal-turn exclusion. Current owner-bound output observation takes precedence when available; existing live history is the fallback. Plan and reasoning drafts now use that same lookup. |
| User and assistant rows | Current shared message renderer retains saved metadata, weather presentation and manager instruction labeling. The role-compatible lookup prevents cross-role enrichment. Restore the streaming copy action. |
| Prompt review | Additive action uses the current work, thread, turn and item. The native prompt-review owner is unchanged. |
| Plan, reasoning and tool rows | Extraction to native_item_content retains labels, attachments, markdown, omission notices, plan copy and activity details. App widgets add the existing open action. |
| Voice and turn boundaries | Retain speech labels, transcript truncation, start/end/failure labels, usage summary, duration and error details. |
| Voice promotions | Retain resolved content or a unique exact turn/item match, attachment rendering, visualization index, truncated status, activity details and unavailable placeholder. Duplicate targets remain unresolved. |
| Attachment captions | Retain original type-to-caption mapping and provider label. |
| Tests | Preserve original live plan/reasoning and provider-identity cases. Restore the removed standalone saved reasoning row and selector assertion. Keep current exact partial-plan replacement and App-widget tests. Add cross-role and streaming-copy regressions. |

## Evidence

The first regression fails before the role fix: native assistant output is
classified as user output for equal text in the same turn. The test now verifies
both saved row orders, correct retained metadata, manager instructions and a
native assistant fallback when only an instruction is available.

The rendered copy regression fails before restoration because the native
streaming response has no copy control. It now clicks the actual control and
reads exact incomplete LaTeX from the test clipboard, both while streaming and
after the live draft is removed.

All 39 native-timeline tests pass. Stable Rust strict Clippy passes for every GPUI
feature and target. Logs:
`/tmp/decodex-native-role-before.log`,
`/tmp/decodex-native-stream-copy-before.log`,
`/tmp/decodex-native-render-after.log`, and
`/tmp/decodex-native-render-clippy.log`.
The initial missing AppContext test import was a compile error, not behavioral
failure evidence; the cited role log contains the actual failing assertion.

## Review boundary

Native author identity and access to existing response text are core correctness.
Weather cards, prompt review, App widgets and animated streaming remain optional
presentation choices with their existing owners. This reconciliation does not
enable automations, introduce a native execution replacement, or qualify a live
provider or signed desktop artifact.
