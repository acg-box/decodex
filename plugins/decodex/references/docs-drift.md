# Decodex Docs Drift Reference

Use this reference when docs claims may diverge from code, commands, config, runtime
state, tracker behavior, validation, or evidence.

## Trigger

Run a drift audit when a lane changes or relies on claims about:

- commands or flags
- schemas or payload fields
- status names or lifecycle states
- config keys
- tracker labels or terminal paths
- validation gates
- prompts or skills
- generated artifacts
- runtime behavior
- research conclusions or promoted decisions

## Audit

1. Scope changed docs and executable surfaces.
2. Extract material claims.
3. Map each claim to direct evidence anchors: source files, tests, CLI output,
   checked-in config, smoke output, or runtime readback.
4. Reverse-check deleted or renamed terms.
5. Decide one verdict: `pass`, `fail`, or `needs-human`.
6. Update the owning concept's `code_refs` and `drift_watch`, or create/update a
   linked `docs/evidence/` drift audit concept.

## Blocking Rule

`fail` blocks ready/review handoff. `needs-human` blocks unless the lane records a
public-safe manual-attention path with the unresolved authority choice.

## Evidence Shape

Use a `type: Drift Audit` evidence concept when the audit needs durable reuse. Keep
private runtime proof in Decodex runtime storage and cite only public-safe summaries
in `docs/`.
