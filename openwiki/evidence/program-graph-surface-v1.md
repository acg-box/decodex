---
type: Reference
title: "Historical Program Graph Surface V1 evidence"
description: "Historical Program Graph Surface V1 evidence"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-477d041b92b25547bc39e55d
    resource: repo://apps/decodex-gpui/src/chief_graph.rs
  - id: openwiki-source-a78ea5fe51f1eae9468e41e0
    resource: repo://apps/decodex-gpui/src/chief_tree.rs
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# Current graph replacement

The Program/Domain lens implementation recorded below is no longer in the active source tree. Its eight former implementation Claims are retired from the current evidence set because their owners (`program_graph.rs`, `factory_surface.rs`, `programs.rs`, and `factory_visual_capture.rs`) were removed. Their old results remain historical prose, not claims about today's graph.

Current graph behavior lives in `chief_graph.rs` and `chief_workspace.rs`. It projects Chief work dependencies and report links; agent ownership is a separate `chief_tree.rs` projection. Tests and capture paths must follow those owners and `bin/workbench_visual_capture.rs`. Do not infer the old cache, lens, keyboard, or fixture guarantees for the replacement graph.

See [Desktop workspace](../architecture/desktop-workspace.md).

---

## Preserved V1 receipt

# Program Graph Surface V1 Evidence

Status: implemented and locally verified.

Program Graph Surface V1 replaces the horizontal Program and Domain Pack strips with a
private, host-owned GPUI component. The component reads only the bounded
`ProgramCycleDto` and `DomainPackProjectionDto` projections. SQLite and `decodexd`
remain the product-state authority. The graph does not store or mutate product facts.

## Ownership and projection boundary

`program_graph.rs` owns one private node, edge, layout, viewport, focus, and rendering
model. It adapts two distinct lenses:

- the Program causal lens contains the Program root, Program nodes, and exact Program
  relation tuples;
- the Domain Pack lens contains the same Program root, Pack-derived entities, and exact
  namespaced relation tuples.

The two lenses reuse layout and rendering mechanics, but they do not merge their
vocabulary or identity sets. The Factory surface owns the surrounding Program pulse,
semantic inspector, causal timeline, evidence controls, Review controls, Pack binding,
and legal Conversation actions.

## Deterministic layered layout

The renderer uses stable node identities and `(from, relation, to)` edge identities as
its structural cache key. It validates empty, duplicate, and dangling inputs before it
creates a scene. The layout sorts identities, removes explicit feedback edges from the
forward rank calculation, deterministically breaks any remaining cycle, assigns the
longest-path rank, and uses bounded barycentric passes for stable row order. Branches
share a layer, merges follow their inputs, and multiple Program cycles continue forward.

`validates` relations are explicit feedback edges. They remain in the scene as dashed
curved paths back to the Program root. The renderer does not present the Program as a
directed acyclic graph. The selected edge set receives a stronger canvas treatment, but
edge color is not the only relation channel.

The layout cache recomputes only when node or edge structure changes. A title, summary,
state, selection, theme repaint, viewport change, or size change can reuse the existing
positions. Static scenes do not start an animation or continuous refresh loop.

## Bounded viewport and interaction

Each lens has a finite world size and an independent bounded viewport. Native controls
provide Fit, zoom out, zoom in, and 100-percent reset. Pointer drag and ordinary scroll
pan the scene. Command-scroll or Control-scroll zooms around the pointer. Zoom and pan
are clamped so the user cannot lose the complete finite scene.

The Graph Surface owns one selected `EntityId`. Graph clicks, keyboard focus, arrow-key
navigation, and causal-timeline activation update that identity. The Factory reads the
same identity for the semantic inspector. If the selected Program node has a
Conversation identity, Enter, Space, and the inspector action open that exact retained
Conversation.

## Keyboard and accessibility behavior

Every graph node is a native focusable GPUI button with a stable element identity,
selected state, semantic kind, title, summary, state, exact identity, and incoming and
outgoing relation text in its accessibility label. Arrow keys choose the nearest node
in the requested spatial direction. Enter and Space activate the selected node. `F`,
`=`, `-`, and `0` operate the active viewport. Viewport controls are also native
keyboard and accessibility actions.

Each lens includes a native relation-readout row for the selected identity. It states
incoming, outgoing, and feedback relations in text. This is the equivalent non-color,
non-pointer path for reading the canvas edges. The graph and Program timeline use no
reveal animation, so reduced-motion use does not depend on a separate animated path.

## Automated and visual evidence

Focused layout tests cover branches, merges, three Program cycles, explicit feedback,
deterministic cycle breaking, stable ordering under reversed input, viewport bounds,
invalid and unavailable projections, and cache reuse. DTO mapping tests prove that the
private scene preserves exact Program and Domain Pack node and relation identities.
GPUI tests cover one shared selection, exact Conversation mapping, unknown-identity
refusal, arrow-key focus navigation, and a complete three-cycle draw at the 1180 by 720
minimum laptop window size.

The visual-capture binary now has two explicit scenarios:

- `development` renders the real three-cycle Development Program shape with three
  Review-to-Program validation paths and the `dev.repository`, `dev.change`, and
  `dev.validation` lens;
- `paper-investment` renders the Paper Investment Pack with two Treasury assets, one
  thesis, one scenario, and the exact comparison, informs, and tests relations.

Both 1490 by 1092 captures were generated after the scene settled and were inspected.
The focused GPUI all-target run passed for the production binary and both capture
targets. Strict package Clippy passed with warnings denied. The schema-11 local database
gate and the 12-test vNext architecture suite also passed. Native signed-app and
accessibility acceptance remain separate completion gates because the capture binary is
test-only and is never staged in `Decodex.app`.

## Retained limits

This surface is a bounded read projection. It does not add a graph database, scheduler,
workflow editor, ontology editor, public scene format, Wasm host, Extension SDK,
arbitrary Pack rendering code, Run Trace facts, provider-attempt facts, or a new product
hierarchy. It does not make the canvas an authoring surface. Normal Program creation,
Pack binding, WorkItem start, Evidence, Review, continuation, and Conversation actions
remain the only legal mutations around the graph.

See [Repeatable Program Loop V1 evidence](repeatable-program-loop-v1.md) for the Program
lineage contract, [Built-in Domain Pack Pressure Test V1 evidence](builtin-domain-pack-pressure-test-v1.md)
for Pack authority and two-domain behavior, and the
[adaptive Program and extension architecture](../decisions/adaptive-program-extension-architecture.md)
for the accepted graph and host-rendering boundary.
