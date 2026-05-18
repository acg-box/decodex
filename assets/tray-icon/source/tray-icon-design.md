# Decodex Menu Bar Icon

Purpose: Source notes for the generated Decodex App menu bar template image.

The menu bar icon uses the same cloud-wrapped prompt-lightning mark as the app icon,
but removes the tile and color. The template variant keeps the cloud slightly smaller
and nudges the lightning mark down and right so the forms stay separated at the 22pt
menu bar size. The lightning uses a small-template polygon to avoid a blocky vertical
edge when the cloud masks it. The internal prompt mark is also reduced slightly and
nudged inward so it does not overpower or hollow out the cloud silhouette. It is
generated as a single-color macOS template image so the system can tint it correctly
in light, dark, and selected menu bar states.

Generator: `scripts/assets/render_decodex_app_icons.swift`.
