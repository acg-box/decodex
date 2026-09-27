# Source attribution

The math recognizer and Unicode layout parser are adapted from OpenAI Codex,
commit `595cc91e8cbb1c2ca822d0311dcf12709410c582`, files
`codex-rs/tui/src/markdown_render/math.rs`, `math/render.rs` and the
`display_width` helper in `codex-rs/tui/src/width.rs`.

The included Apache 2.0 license applies to this derived code. Decodex changes
module paths, iterator plumbing and desktop rendering integration. Upstream
parser limits and literal fallback rules are retained.

## Upstream notice

OpenAI Codex
Copyright 2025 OpenAI

This project includes code derived from [Ratatui](https://github.com/ratatui/ratatui), licensed under the MIT license.
Copyright (c) 2016-2022 Florian Dehau
Copyright (c) 2023-2025 The Ratatui Developers
