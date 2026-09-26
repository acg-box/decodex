# Native MCP form coverage

Restore the inherited cases for standard form and URL requests with approval
metadata. Restore the unsupported user-verification case and its fixture modes.
No production code or native capability declaration changes in this batch.

## Native evidence

Both tests pass with installed Codex `0.158.0-alpha.2` against an isolated local
Responses provider and stdio MCP server. Each case has a private temporary
HOME, CODEX_HOME and database. Child environments contain no account credentials.
Each native process completes shutdown before its directory is removed.

The two tests cover six cases:

- An OpenAI form accepts the schema's wire value, rejects its display label,
  records the decision and rejects a second answer.
- An opaque schema cannot be accepted blindly; cancellation remains available.
- Native `never` policy declines the form without creating a Chief request.
- Undeclared user verification returns native error `-32602`, creates no Chief
  form and returns no verification proof.
- A standard form with approval metadata reaches Chief and records a decline.
- A URL request retains the exact URL and records a decline. It does not open
  the URL or prove browser-based completion.

Each case completes exactly two synthetic inference requests. Standard form and
URL cases each forward exactly one native request and reject a duplicate reply.
Strict runtime lint passes with all features and targets.

The fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582` has distinct
form and user-verification capability gates in
`codex-rs/rmcp-client/src/elicitation_client_service.rs`. Decodex declares the form
extension only. The live MCP initialization receipt checks that exact declaration.
Keep unsupported verification owned by the native rejection path; do not build a
local verification substitute or advertise support that is not implemented.

## Scope

The Python server matches the complete preserved file. The Rust test file differs
only in explicit imports. Both original hashes were checked. This restores test
coverage for existing behavior, not a new product feature. Service/native proof
does not establish rendered form, browser, accessibility or final desktop
acceptance. Other R09 consumers remain open.
