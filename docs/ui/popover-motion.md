# Popover motion

## One owner

Use `ui_motion::popover` for in-window menus. Response usage uses a fixed
anchored card. Keep each card in one subtree.
Render the fill, shadow and content in the same deferred subtree.

GPUI at `92f315647f776854053fc334b73110d97964bc5f` applies `Div::opacity`
separately to paint primitives (`Window::with_element_opacity`, `paint_quad`,
`paint_drop_shadows`, and `paint_glyph`). It does not first composite a card into
one surface. Do not treat it as whole-card alpha. Our in-window cards remain
opaque at their final anchor. The interim translated-card animation was rejected
in live review. Do not restore it by tuning speed or travel. In-window menus
currently open and close directly; a true whole-surface fade remains unfinished.
Do not describe this fallback as a finished animated transition.

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

The native timeline presentation test checks that usage details disappear on
hover exit and preserve the footer bounds.
