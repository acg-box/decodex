---
type: Reference
title: "Historical Program Graph Surface V1 evidence"
description: "Historical Program Graph Surface V1 evidence"
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-2a6989c7fa25b9681df71f95
    resource: repo://apps/decodex-gpui/src/agent_collaboration.rs
  - id: openwiki-source-c222de0005be9c363ba17232
    resource: repo://apps/decodex-gpui/src/agent_relation_canvas.rs
  - id: openwiki-source-51a4755f3c4ddd78511e5c8e
    resource: repo://apps/decodex-gpui/src/agent_tree.rs
  - id: openwiki-source-c0e58bc836ac05c982449723
    resource: repo://apps/decodex-gpui/src/agent_workbench.rs
generated: { by: "codex", at: "2026-10-21T14:42:59.992Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-24T21:11:31.338Z
---

# Current graph boundary

The former Program and Domain Pack graph is retired. The current Agent graph projects work relationships. `agent_tree.rs` places the right Collaboration panel, which groups recorded exchanges and pending response requests across chats. It no longer renders a selected-work inspector or an ownership-only tree.

Current owners include `agent_workspace.rs`, `agent_relation_canvas.rs`, `agent_collaboration.rs` and `agent_workbench.rs`. The former owners (`program_graph.rs`, `factory_surface.rs`, `programs.rs` and `factory_visual_capture.rs`) are absent. Their old cache, lens, keyboard and fixture guarantees do not qualify the replacement.

The [original V1 implementation and test receipt](https://github.com/acg-box/decodex/blob/426e862a5978620ffad65fb3d0189cfa57392873/openwiki/evidence/program-graph-surface-v1.md) remains in Git history. Its test counts, captures and `decodexd` ownership statement describe that historical implementation. This refresh did not rerun them or establish installed-app acceptance.

See [Desktop workspace](../architecture/desktop-workspace.md) for current presentation and [Agent coordination](../architecture/chief-coordination.md) for work ownership.
