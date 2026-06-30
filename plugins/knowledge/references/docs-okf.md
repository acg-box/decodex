# Docs OKF

Use for strict `docs/`; use `okf-layer.md` for portable OKF. `docs/` is
Markdown-only; dirs need `index.md`; roots are `docs/index.md`, `docs/policy.md`,
`docs/log.md`; concepts use frontmatter; prose spells `OKF`.

Frontmatter needs required identity/status fields; types: `Decision`, `Drift Audit`,
`Evidence`, `Policy`, `Reference`, `Research Contract`, `Runbook`, `Spec`; routing
fields: `tags`, `source_refs`, `code_refs`, `related`, `promotes_to`, `drift_watch`.

`Research Contract`: Question, Scope, Evidence, Options, Judgment, Challenge, Decision, Promotion, Drift Impact, Citations. `Drift Audit`: Watched Claims, Evidence Anchors, Reverse Checks, Verdict, Required Updates, Citations; `Verdict` is `pass`, `fail`, or `needs-human`. Validate with `decodex docs check`.
