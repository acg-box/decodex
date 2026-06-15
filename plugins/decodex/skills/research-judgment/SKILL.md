---
name: research-judgment
description: Use after Decodex evidence and option comparison to form a challenge-ready recommendation or an explicit not-decision-ready conclusion.
---

# Decodex Research Judgment

## Goal

Turn evidence and options into a testable conclusion without skipping unresolved
uncertainty.

## Judgment Shape

A challenge-ready judgment must include:

- recommended option or explicit non-decision outcome
- why this option best satisfies the decision criteria
- evidence ids, source references, or code references that support it
- important assumptions and constraints
- rejected alternatives and why they lost
- unresolved evidence gaps, if any
- expected validation if promoted into execution

For machine-authored runs that need replayability, assign a stable judgment id or hash
over the normalized conclusion and cited evidence. The Decision Contract remains the
Decodex authority boundary.

## Boundaries

- Do not call a judgment final before skeptic challenge.
- Do not hide weak evidence behind confident language.
- Do not mark the result `decision_ready` if the judgment depends on unresolved human
  direction, unavailable evidence, or untested assumptions that materially affect the
  chosen option.
