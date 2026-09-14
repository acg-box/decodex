# Decodex App Icon

The default is the open-cloud Liquid Glass design (`03-open-cloud`).
`assets/app-icon/default-variant` selects the production icon. All three approved
candidates remain in `assets/app-icon/liquid-glass` for comparison and editing.

The shared geometry and material source is
`scripts/assets/build_liquid_glass_icons.swift`. It generates clean SVG layers and
matching menu-bar templates. The cloud frame is behind the lightning and cursor.
The Dock glyphs share an optical baseline; the open-cloud glyph group is reduced
to 86% and shifted up 12 source points within its frame. The complete mark is
centered from its outer bounds. The menu bar has a separate, heavier optical fit
for its 22-point display size.

Run `swift scripts/assets/render_decodex_app_icons.swift` from the repository root
to refresh the default previews, static compatibility export, and menu-bar template.
The stage script compiles the selected native `.icon` into `Assets.car` and copies
its matching menu-bar template. The compiler checks native vector icon stacks for
Default, Dark, and Mono. The ICNS is a fallback, not the source of Liquid Glass.

See `../liquid-glass/README.md` for material settings, trial packaging, and system
appearance review. `../approved` contains the selected visual references only.
