---
name: research-options
description: Use after Decodex research evidence collection to compare realistic implementation, architecture, or policy options with evidence-grounded tradeoffs before judgment.
---

# Decodex Research Options

## Goal

Force a real choice. A Decodex research answer is not decision-ready merely because one
plan sounds plausible.

## Option Comparison

Include realistic options such as:

- keep current behavior or status quo
- minimal patch
- architecture-level redesign
- staged migration
- explicit no-go or defer outcome

For each option, record:

- what changes
- evidence supporting it
- tradeoffs and risks
- what it would make easier
- what it would make harder
- why it is selected or rejected

## Decision Contract Mapping

Use `research_options` for option records. A `decision_ready` result must have at least
one realistic option comparison and should preserve rejected alternatives when they
would otherwise be rediscovered later.

## Boundaries

- Do not compare straw-man options.
- Do not select an option without tying the decision to evidence or explicit
  assumptions.
- Do not expand into executable issue briefs until the selected option survives
  judgment and challenge.
