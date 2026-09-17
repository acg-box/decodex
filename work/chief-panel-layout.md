# Chief panel layout

## Behavior

- Keep the conversation in the center.
- Put the work graph in a bottom dock. Use horizontal flow for both dock and expanded views.
- Derive dock height from node bounds and zoom. Limit it to 360 logical pixels and 45% of the available height. Keep at least 240 logical pixels for the conversation where the window permits it.
- Keep the graph camera independent of panel size. Pan must not resize the dock. Large graphs remain accessible with pan and zoom.
- Expanded graph uses the full center area. The graph does not change the selected conversation or its history navigation preference.
- Use the right sidebar for the real agent ownership tree. Parent arrows expand and collapse branches; labels open existing conversations. Work status is secondary text. Tree visibility and collapsed branches survive navigation.
- Center the history tick group on the transcript edge. Hover magnifies nearby ticks with an eased transition. Navigation animates a bounded scroll offset and keeps 56 logical pixels of preceding context where available. Wheel scrolling cancels navigation and applies each delta once. Conversation contents remain a continuous document.
- Use the bottom-panel toolbar icon for Graph and the clock icon for the history rail (Command-J).
- Let the user drag the left sidebar edge. Limit width to 160–360 logical pixels, subject to available window width. Keep the width when the sidebar closes or the selected work changes. Double-click the edge to reset. Arrow keys change width when the edge has focus.
- Keep direct pointer tracking for resize and animated panel visibility. Keep the existing glass materials.

## Validation

- GPUI all-target tests: 156 and 154 passed; 4 and 2 existing ignored tests.
- Geometry tests cover content limits, small windows, expanded bounds, and sidebar limits.
- GPUI pointer test covers drag, release, and unchanged graph camera. History navigation has a native layout test with unequal message positions.
- Strict GPUI Clippy passed.
- Native capture review: `target/visual-tests/chief-history-preview.png` and `target/visual-tests/chief-compact-metrics.png`.
- Capture data is test-only. The production application uses service records.

## Conversation details

Use compact K and M units for token and context counts. Hide context metadata when
no nonzero context usage has been reported. Align user message rows to the right
with a content-sized bubble capped at 78% of the conversation width. Keep assistant
Markdown across the conversation column.

## Density and materials

Shared tokens define 12.5-point body text, 10.5-point captions, 19-point body line
height, 28-point tree rows, and 20-point message spacing. Reply metadata uses
separate items and separators with an equal six-point gap. The user rejected the separated background treatment. Restore the shared main
content tint beneath the conversation, dock, and tree. Keep the existing material
colors and opacity values.

Native verification covered the existing Chief, the final queued-recovery reply,
and preceding report content in one continuous transcript. Geometry tests cover
short and long Chinese user bubbles at multiple widths. Wheel tests cover both
directions and exact single application of the input delta.

## Dock and tree transparency correction

Keep the shared main content material and the left sidebar unchanged. Remove only
the additional full-area tint from the graph dock and agent tree. Both panels now
show the existing translucent main surface directly. Borders and node surfaces
still distinguish the panels. Do not move the main material back onto the chat.

Use a one-point boundary at 17% white opacity for the dock top and tree left edge.
Use a lighter 7% white rule below both 30-point panel headers. These lines define
the panel limits without adding another full-area material.

The agent tree header uses 13-point semibold text above 12-point node labels.
The global top control opens and closes the tree; the header has no duplicate
close button.

History ticks use at most 11-point spacing. Command-J toggles the graph,
Command-E toggles the left sidebar, and Command-B toggles the right agent tree.
The history rail remains available from its toolbar icon.
