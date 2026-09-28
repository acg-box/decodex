# Turn process disclosure

## Native source

Reference: openai/codex `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`codex-rs/app-server-protocol/src/protocol/v2/item.rs`, `ThreadItem::AgentMessage`.
The native message has an optional phase. Schemas generated from the installed
`codex-cli 0.158.0-alpha.2.1` also contain `commentary` and `final_answer`.
This is a presentation adapter, not a replacement for native history.

The public timeline preserves the optional phase. Missing and unknown phases do
not become final answers. This additive field defaults to absent for older peers.
No stored history is rewritten.

## Presentation

- Keep active, failed, and interrupted turns open.
- Fold process items only after a successful terminal boundary and an explicit,
  nonempty final answer are present on the loaded page.
- Keep user input, final answers, unknown message phases, attachments, and App UI
  entries visible. Fold commentary, public reasoning summaries, plans, and terminal
  tool activity behind one `N earlier messages` control per native turn. Place the
  control immediately before the final answer. No private reasoning is requested.
- Retain explicit expansion during refresh. Clear expansion when the binding changes.
- If the reader is browsing history, keep newly completed processes open.
- Keep the control anchored during manual toggles. Invalidate measured row heights
  when the folded set changes; do not reuse heights from a different layout.
- Use compact process typography and specific tool names. The detail view retains
  raw type, turn and call identities, formatted input and full paginated public output.
  Existing sensitive-content filtering still applies.

The conservative fallback for old records without phase metadata is expanded
history. Do not guess a final reply from the last assistant message.

## Manual disclosure motion

Process sections and tool details use a centered, vector-drawn chevron that
rotates 90 degrees over the shared 200 ms transition. The content reveals its
measured height at its original position and stays mounted through closing. Reversals
start from the current height. Respect reduced motion.

Construct process content lazily, only during expansion or closing. Once open,
use natural height so nested tool details do not receive a second delayed height
animation. Give each disclosure an identity from its native work, turn and item.
Split process segments around interleaved input or interactive content to preserve
source order. These segments share one total count and one control. Steer input
belongs to its native turn and stays visible. Do not move user input into a
collapsed process. Keep the final-answer anchor stable throughout disclosure motion.
