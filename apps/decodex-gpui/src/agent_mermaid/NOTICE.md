# Source attribution

The closing-fence check in `agent_mermaid_view.rs` is adapted from OpenAI Codex,
commit `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`codex-rs/tui/src/markdown_render/mermaid.rs`. See `LICENSE-APACHE`.

OpenAI Codex
Copyright 2025 OpenAI

CSS variable resolution in `svg.rs` follows the terminal adapter in
[wevm/ondine](https://github.com/wevm/ondine/blob/6a1586e5af980e67c104239593ae931837157b8b/src/cli/Terminal.ts),
commit `6a1586e5af980e67c104239593ae931837157b8b`. Decodex resolves presentation
attributes only, preserves authored text, and applies its desktop palette.
See `LICENSE-ONDINE`.

The diagram fixtures in `fixtures.json` use the first example for each diagram
type from Ondine's documentation at the same commit. See `LICENSE-ONDINE`.
