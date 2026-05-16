# Decodex App Icon

Purpose: Source notes for the generated Decodex App icon.

The app icon uses a purpose-built Decodex/Codex mark: a large modern cloud prompt with
a lightning mark entering from the right edge. The `>_` prompt connects it to Codex,
while the lightning suggests quick account switching and active control without relying
on text.

The flat `.icns` output is a local-development export; the composer lane keeps the
foreground layer separate so an Icon Composer pass can rebuild the same
prompt-cloud-bolt mark as a layered Liquid Glass icon package. Treat the checked-in
`.icns` as the packaged flat fallback, not the final multi-layer Liquid Glass artifact.

Generator: `scripts/assets/render_decodex_app_icons.swift`.
