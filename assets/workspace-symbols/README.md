# Workspace toolbar symbols

These PNG assets are native macOS SF Symbols for the Decodex macOS interface.
The renderer uses a regular 14-point symbol in a 16-point box, sampled at 3x.
GPUI embeds the images in the executable; no asset lookup depends on the checkout.

| Asset | System symbol | Action |
| --- | --- | --- |
| sidebar.png | sidebar.left | Toggle the left sidebar |
| graph.png | sidebar.right | Toggle the work graph panel |
| timeline.png | rectangle.bottomthird.inset.filled | Toggle the bottom timeline |
| expand.png | arrow.up.left.and.arrow.down.right | Expand or restore the graph |
| settings.png | gearshape | Open settings |
| close.png | xmark | Close a panel or work tab |
| plus.png | plus | Zoom in |
| minus.png | minus | Zoom out |
| back.png | arrow.left | Back or parent graph scope |
| forward.png | arrow.right | Forward in navigation history |
| send.png | arrow.up | Send to Chief |

Regenerate on macOS with the host Swift toolchain:

```sh
swift scripts/macos/generate_workspace_symbols.swift assets/workspace-symbols
```

The generator fails if the host does not provide a named system symbol. It does
not install a font, package, runtime or toolchain.
