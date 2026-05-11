# Static Public Site

Status: accepted

Date: 2026-05-09

Question: Should the public Decodex site become a dynamic app now that the runtime and
site live in one mono repo?

Decision: Keep the public site static by default. `site/` remains an Astro static site
that renders checked-in content and generated JSON from the GitHub signal pipeline.
Runtime orchestration, local operator state, tracker writes, app-server integration, and
the operator dashboard stay in `apps/decodex/` and the local `decodex serve` control
plane.

Consequences:

- Public content remains diffable, reviewable, cacheable, and deployable through GitHub
  Pages without a live Decodex daemon.
- `scripts/github/` remains the content-generation script boundary for public signals
  and release deltas, with checked-in GitHub bundles and editorial analysis drafts
  under `artifacts/github/`.
- `apps/decodex/` can evolve the runtime without turning the public website into an
  operational dependency.
- Dynamic public capabilities such as login, personalized feeds, live queries, or
  paid/private access require a later decision before the site depends on a backend.
