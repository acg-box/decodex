# Decodex Docs Drift Reference

Use this when Decodex docs claims may diverge from code, commands, config, runtime
state, tracker behavior, validation, or evidence. Portable OKF graph and frontmatter
quality checks use `okf-layer.md` and `decodex okf`.

## Trigger

Audit claims about commands, flags, schemas, statuses, config keys, tracker labels,
terminal paths, validation gates, prompts, skills, generated artifacts, runtime
behavior, or promoted research.

## Audit

1. Scope changed docs and executable surfaces.
2. Extract material claims.
3. Map each claim to source evidence: code, tests, config, CLI output, or runtime
   readback.
4. Reverse-check deleted or renamed terms.
5. Record `pass`, `fail`, or `needs-human`.
6. Update `code_refs`, `drift_watch`, or a linked `docs/evidence/` drift audit.

`fail` blocks ready/review handoff. `needs-human` blocks unless the lane records a
public-safe manual-attention path.

## Helper

Use `plugins/decodex/scripts/semantic_drift_audit.py` from the repository root when a
git diff packet can reduce manual scanning. It reports changed docs, changed
executable surfaces, added claim-like lines, removed executable terms, stale phrase
hits, and whether agent review is required. The helper is not a verdict; compare the
candidate packet against direct evidence before reporting `pass`, `fail`, or
`needs-human`.
