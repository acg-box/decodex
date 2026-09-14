# Decodex App Icon

The default is the pixel-dissolving cloud cutout Liquid Glass design
(`01-mercury-cloud`). Both Dock and menu bar use this reference-led cloud with lightning
and cursor cutouts. Dock adds 15 square layers on a shared grid, with dense overlays at the cloud
edge and sparse pale pixels beyond it. The menu bar uses the same 15-cell grid as a solid system template; its cutouts
have a small-size optical fit. Dedicated 22px and 44px representations avoid
repeated downsampling of tiny pixels.
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

The default full-color appearance uses a solid white background and a cyan-blue
cloud, with pixels fading toward pale blue. Native glass adds its own subtle
lighting to the white surface. Dark, Clear, and Tinted annotations are separate;
this does not change the menu-bar template or the user's global appearance.

## Refined Dock contour

The pixel-cloud foreground uses a uniform 1.18 scale with the approved optical offset. The left shoulder has a short curved transition. The right cloud cap follows a circular arc and meets the last pixel row without overlapping its glass face. The cloud retains its flat base, asymmetric silhouette, and lightning and underscore cutouts. Menu bar geometry remains sized for its 22-point surface.
