# Popover motion

## One owner

Use `ui_motion::popover` for in-window menus and `popover_progress` for anchored
cards such as response usage. Keep the content mounted until the exit ends.
Render the fill, shadow and content in the same deferred subtree.

GPUI at `92f315647f776854053fc334b73110d97964bc5f` applies `Div::opacity`
separately to paint primitives (`Window::with_element_opacity`, `paint_quad`,
`paint_drop_shadows`, and `paint_glyph`). It does not first composite a card into
one surface. Do not treat it as whole-card alpha. Our in-window cards remain
opaque and move four pixels, with a 140 ms entry and a 90 ms exit. This avoids
a faint-content phase that can leave a dark surface visually dominant.

Native notification windows have a different boundary: `native_presence` drives
`NSWindow::alphaValue` for the complete window. Keep that path separate.

## Frame scheduling

Use `ui_motion::request_frame`. Do not add a local `on_next_frame` callback that
refreshes the whole window. GPUI throttles inactive windows, while an attached
glass composer can own keyboard focus. The shared helper schedules through that
child when needed.

## Regression check

Check the model palette and response-usage hover card with the composer focused
and unfocused. Check entry, exit, and quick reversal. Text, fill and shadow must
appear and disappear together. The conversation must not move. Check native
notifications separately; a passing static screenshot does not prove animation.

The native timeline presentation test checks that usage details keep their
content during exit, disappear after the exit, and preserve the footer bounds.
