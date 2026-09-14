# Native Liquid Glass icon trials

The production default is `03-open-cloud`, selected by `../default-variant`.
All three variants are retained. These three variants use native Icon Composer materials. The SVG layers contain
white geometry only. They do not contain rendered lighting, texture, or a complete
flattened icon image.

## Geometry authority

`scripts/assets/build_liquid_glass_icons.swift` defines the shared geometry with
circles, rounded rectangles, and vector boolean operations. It generates editable
SVG source and the matching menu-bar templates. Do not trace photographic
highlights into the outline.

The rounded cloud has an offset main peak and unequal shoulder sizes. This avoids
a rocket-like symmetric silhouette. The flat and open variants keep balanced
geometry.

The open cloud has a 72-point frame. Its right cap is aligned to the normal of the
frame's circular arc, so the cap and frame meet tangentially. The lightning and
cursor share a baseline. Within the open frame, both use an 86% scale and a
12-point upward offset. This leaves about 54 source points below the cursor. The complete Dock mark is
centered vertically from its outer bounds, so this internal correction does not
move the whole icon upward.

The menu bar uses a separate optical fit for its 22-point display size. The open
frame is inset by 6 source points to reduce its weight. Its glyphs use 97% width,
82% height, and a 3-point weight expansion. The group moves 30 points right and
8 points up to leave room at the upper-left shoulder and bottom edge. The filled
clouds retain their 110% fit and 7-point weight expansion.

`check_menu_icon_legibility.swift` verifies the exported open-cloud PNG at 22 and
44 pixels, at both 25% and 50% alpha thresholds. The frame, lightning, and cursor
must remain three separate connected components, including antialiased edges.
The generator runs this check automatically. Dock geometry is unaffected by the
menu-bar correction.

## Native materials

| Variant | Refraction depth | Refraction strength |
| --- | --- | --- |
| Rounded cloud | 22% | 10% |
| Flat cloud | 18% | 10% |
| Open cloud | 14% | 10% |

All use inside specular highlights and native neutral shadows. Default, Dark, and
Mono have separate fill, translucency, and shadow values. Mono supports Clear Light,
Clear Dark, Tinted Light, and Tinted Dark. The system supplies dynamic rendering;
these are authored presets, not a promise that every arbitrary material setting
will produce the same appearance.

The open cloud frame is behind the lightning and cursor. This prevents the frame
from refracting the glyphs into its lower edge. Menu-bar icons remain monochrome
system templates and share the base geometry with a size-specific optical fit.

## Build and review

```sh
swift scripts/assets/build_liquid_glass_icons.swift
python3 scripts/macos/prepare_decodex_icon_trials.py /absolute/path/to/new-trial-folder
```

Use `--base-app /path/to/Decodex.app` to package a freshly built application.
The default baseline is the installed application.

The packager invokes `actool`, checks that `Assets.car` contains vector artwork and
native icon stacks for Default, Dark, and Mono, then signs each local trial. It sets
`CFBundleIconName=AppIcon` and a separate trial build number for each icon revision
to prevent old icon-cache entries from being reused.

Use `render_liquid_glass_appearance.swift` for a system-rendered preview. On this
macOS host the process-local theme values are RegularLight, RegularDark,
ClearLight, ClearDark, TintedLight, and TintedDark. These override only the review
process; they do not write the user's global appearance settings.

```sh
swift scripts/assets/render_liquid_glass_appearance.swift /path/Decodex.app /path/review.png -AppleIconAppearanceTheme ClearDark
```

Inspect both a 1024-pixel render and Dock-size previews. Check the right cap,
inside glyph corners, bottom spacing, and contrast. Review images belong outside
`AppIcon.icon`; do not ship them as artwork layers.
