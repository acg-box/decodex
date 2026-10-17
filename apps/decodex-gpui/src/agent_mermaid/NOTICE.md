# Source attribution

The bounded Mermaid parser and layout are adapted from OpenAI Codex, commit
`595cc91e8cbb1c2ca822d0311dcf12709410c582`, `codex-rs/mermaid/src/`.
The closing-fence check is from `codex-rs/tui/src/markdown_render/mermaid.rs`.
The included Apache 2.0 license applies to this derived code.

Decodex changes module paths, formatting, test assertions and desktop integration.
Parser grammars, resource bounds and complete-source fallback remain upstream-owned.

## Upstream notice

OpenAI Codex
Copyright 2025 OpenAI

This project includes code derived from [Ratatui](https://github.com/ratatui/ratatui), licensed under the MIT license.
Copyright (c) 2016-2022 Florian Dehau
Copyright (c) 2023-2025 The Ratatui Developers

Stadium node parsing and rounded-corner rendering follow upstream commit
`2426ed7684c87f9a627c60b54271cfed77c979df`.

Entity escape rejection before statement splitting follows upstream commit
`659b35f1316eda27ef61850dd0832c4a4e95c120`. The broader label grammar from
that commit is not included.

Multiline flowchart node labels and their drawing follow upstream commit
`9fcb1f5ed49c938b6fa5a9b4c8f897f3ddfd734c`. This includes the three explicit
break tags and quoted node labels within the existing strict label subset;
the broader punctuation and edge grammar is not included.
