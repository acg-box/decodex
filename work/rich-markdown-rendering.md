# Rich Markdown rendering

## Scope

Adapt the bounded math and Mermaid parsers from OpenAI Codex at fixed commit
595cc91e8cbb1c2ca822d0311dcf12709410c582. Source attribution and licenses are in
chief_math/ and chief_mermaid/ under the GPUI source directory.

Math uses a masked rendering copy to preserve TeX source offsets. Code, links,
HTML, currency, incomplete expressions, and unsupported expressions retain the
upstream exclusions or literal fallback. Display formulas keep fixed spatial
rows with horizontal scrolling. Formula copy returns original TeX.

Completed Mermaid fences render the upstream subset of flowcharts, sequences,
flat states, classes, and entity relationships. The parser limits source size,
node and edge counts, labels, canvas area, and output width. Unsupported or
incomplete diagrams remain code. No diagram code executes or loads web content.

Response copy includes Markdown text and HTML. Formula and diagram HTML retain
source rather than replacing it with rendered Unicode. HTML escapes content and
only creates web links for HTTP(S) URLs. Native pasteboard writes preserve plain
text and do not append markup after another application replaces that text.
Existing weather marker handling and weather card copying remain in place.

## Dependency evidence

Selection scope: add a direct GPUI reference to existing unicode-width 0.2.2,
with workspace requirement ~0.2. Offline resolution changes only the GPUI edge.
All lock package identities and checksums are unchanged. The cached crate archive
matches the lock checksum. No new transitive package or build script is introduced.

Fresh baseline and final cargo-audit scans on 2026-09-25 both report the existing
rustls 0.23.40 advisory RUSTSEC-2026-0285 and the same warning categories. Neither
scan reports unicode-width. This batch does not fix that existing advisory or
claim that the dependency graph is free of vulnerabilities. Scoped risk_coverage:
complete for graph identity, artifact integrity, and RustSec comparison;
risk_delta: unchanged; decision: proceed with the existing dependency edge.
Evidence: /tmp/decodex-rich-audit-before.json and decodex-rich-audit-after.json.

## Validation boundary

The focused Markdown suite passes 32 tests, including source fallback, exhaustive
three-node graph routing, resizing, horizontal scrolling, formula/source copy,
native pasteboard format retention, and existing weather cards. Full desktop and
strict Clippy results are recorded with the delivery PR. Rendered test fixtures
are not signed whole-application visual acceptance.
