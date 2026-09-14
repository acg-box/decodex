# Decodex Icon Family

The Dock and menu-bar icons share one mark: cloud, terminal prompt, and lightning.

## Source authority

`scripts/assets/render_decodex_app_icons.swift` owns the geometry for both surfaces:

- `cloudPath()` defines the cloud silhouette.
- `promptCenterlines()` and `TemplateMark.promptWidth` define the prompt.
- `templateBoltPoints()` defines the lightning.
- `TemplateMark` defines the relative positions and scales.

`sharedMarkPaths()` exports those same paths for the Dock icon. It merges the
cloud contours and expands the prompt strokes into filled paths. The common Dock
transform changes the overall scale and placement only. The cloud remains the
horizontal visual anchor; the lightning does not move it to the left.

The four SVG files in `assets/app-icon/composer/AppIcon.icon/Assets` are generated
outputs. Do not edit their paths by hand. Change the shared Swift source and run:

```sh
swift scripts/assets/render_decodex_app_icons.swift
```

This command refreshes the SVGs, static ICNS, PNG previews, size review, and
menu-bar template. It uses the repository's Xcode toolchain to render the Dock
icon through Icon Composer's asset compiler.

## Surface treatment

The menu-bar icon uses a black template with a clear prompt. macOS supplies its
foreground color. The Dock icon uses a pearl-blue cloud, dark prompt, amber
lightning, and a navy background.

Both use the same overlap: prompt above cloud, lightning behind cloud. The cloud
is opaque so the hidden lightning cannot show through the prompt. Cloud and
background retain system highlights. The prompt and lightning disable specular
highlights to keep their contours clear at small sizes.

`icon.json` owns the Dock colors, material settings, and layer order. Keep groups
ordered front to back: prompt, cloud, lightning.

## Build and review

The app stage script calls `scripts/macos/compile_decodex_app_icon.sh` to compile
`AppIcon.icns` and `Assets.car` into the bundle. `CFBundleIconName` selects the
layered asset; `CFBundleIconFile` names the static fallback. The compiler checks
both names against Apple's generated partial Info.plist.

Inspect 32, 64, 128, and 256 pixel previews on light and dark backgrounds. Check
that the prompt stays clear, the lightning does not cover the cursor, and the
cloud keeps its visual center. PNG previews do not prove the installed Dock's
runtime appearance selection; validate that separately after installing a build.
