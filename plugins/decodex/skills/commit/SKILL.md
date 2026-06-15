---
name: commit
description: Use when Decodex must commit a human lane.
---

# Commit

Create human-driven Decodex commits through `decodex commit`. Read
`../../references/routing.md` for the full commit boundary.

1. Inspect the diff and stage only intended files.
2. Run the touched surface's validation when useful.
3. Run `decodex commit "<summary>"`, or
   `decodex commit --manual-authority "<summary>"` for non-issue work.

Do not substitute raw `git commit`, and do not use this skill for PR creation,
landing, cleanup, retained-lane automation, or runtime-owned orchestration.
