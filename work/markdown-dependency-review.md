# Native Markdown dependency selection

Scope: add pulldown-cmark to the GPUI conversation renderer. Keep the existing
GPUI revision. The baseline had no Markdown parser dependency.

## Resolved delta

- pulldown-cmark 0.10.3, crates.io, direct requirement ~0.10, default features off.
- unicase 2.9.0, crates.io, new transitive dependency.
- bitflags and memchr reuse their locked versions.
- time uses the existing workspace dependency and locked version for date labels.
- Cargo generated the lockfile. No unrelated package identities changed.

## Evidence observed 2026-09-16

- OSV package/version queries for both new packages: checked-no-record.
  Source: https://api.osv.dev/v1/query . This is package-scoped advisory coverage,
  not a claim that the entire existing dependency graph has no vulnerabilities.
- crates.io version metadata and local crate archive SHA-256 match the generated
  lockfile checksums: checked-no-record for identity/integrity conflicts.
- Source identity: pulldown-cmark/pulldown-cmark and seanmonstar/unicase repositories
  declared in the crates.io source artifacts. Inspected local manifests and build
  behavior. The parser build script is a no-op without gen-tests; that feature is
  disabled. unicase declares build=false. No runtime download is introduced.
- Separate ecosystem malware attestation: unsupported for these crate artifacts.

## Decision

risk_coverage: scoped registry, artifact, source/build behavior, and OSV checks.
risk_delta: two new parser-related packages; no changes to existing versions.
decision: proceed with the bounded native renderer. No adverse record found in
the checks above; this is not a general security certification.

The renderer treats HTML as text, does not fetch images, and only opens supported
links after a click. Source-file links reveal the local file.
