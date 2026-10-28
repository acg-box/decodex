# Source attribution

The math recognizer is adapted from OpenAI Codex commit
`595cc91e8cbb1c2ca822d0311dcf12709410c582`, file
`codex-rs/tui/src/markdown_render/math.rs`, with the inline zero fix from
`7b7d934408e6df20950a7a2501a669650279ab06`.

The Unicode layout parser is adapted from commit
`33d306d457a4632b22bebf214e0c25d91f90f6e8`, files
`codex-rs/tui/src/markdown_render/math/render.rs` and `math/render/structured.rs`.
The `display_width` helper comes from `codex-rs/tui/src/width.rs`.

The included Apache 2.0 license applies to this derived code. Decodex changes
module paths, iterator plumbing and desktop rendering integration. Upstream
parser limits and literal fallback rules are retained.

## Upstream notice

OpenAI Codex
Copyright 2025 OpenAI

This project includes code derived from [Ratatui](https://github.com/ratatui/ratatui), licensed under the MIT license.
Copyright (c) 2016-2022 Florian Dehau
Copyright (c) 2023-2025 The Ratatui Developers
