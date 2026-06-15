---
name: manual-cli
description: Use when a human drives Decodex CLI.
---

# Manual CLI

Assist human-driven Decodex CLI work without taking over retained automation. Read
`../../references/routing.md` for commands and recovery boundaries.

- Use installed `decodex` for installed runtime work.
- Use `cargo run -p decodex --bin decodex -- ...` in this repository.
- Treat `run --dry-run` as planning evidence, not proof of live writes or closeout.
- Before live `decodex run`, read `automation` and project `WORKFLOW.md`.
- Do not hand-edit runtime DB state or clean retained worktrees from the side.
