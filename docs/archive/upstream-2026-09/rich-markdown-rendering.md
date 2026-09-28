> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

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

## Complete inherited Markdown review on 2026-09-27

The original math and Mermaid documents are consolidated here. Their fixed-cutoff
source attribution, literal fallback, resource limits and desktop acceptance
boundary remain applicable. Their old “unmerged” status and vulnerability totals
are historical. The dated dependency evidence above supersedes those old totals;
this review makes no new dependency selection or current clean-bill claim.

Fresh byte comparison finds 23 inherited math/Mermaid source, notice, license and
test files unchanged. The remaining Mermaid test file changes only the layout of
one function call; all assertions remain. The register now records that formatting
difference instead of claiming exact bytes.

Math retains 4 KiB input, depth 32, 16 layout rows and 256 columns. Nested display
fractions and unsupported expressions use source fallback. Display rows remain
spatial under horizontal scrolling. Copy retains original TeX. Mermaid retains
16 KiB source, 16 nodes, 24 edges, 16 members, eight sequence participants,
64 sequence events, four fragment levels, 40-cell labels, 65,536 canvas cells and
256 output columns. Comma-delimited fence information remains recognized; EOF
without a closing fence remains source code.

The complete chief_markdown.rs comparison has two functional areas: restored
local file location handling and the retained weather fallback. Weather parsing
preserves code spans/blocks, unknown markers and original stored text. Its prefix
tests and rendered forecast-copy test remain. Existing parser, layout, copy and
selection behavior is otherwise preserved; fixture DTO fields follow current
native-source, turn and weather metadata.

The current local link handler removed only one numeric suffix. A restored
inherited regression fails for /tmp/中文.rs:12:3, revealing /tmp/中文.rs:12.
Restore the original two-suffix helper and use it in the actual selectable-text
click handler. Retain Unicode paths, colons inside filenames, invalid suffixes,
and the complete Markdown link target. This reveals the file; it does not promise
an editor jump to that line and column.

The 33-test Markdown suite passes, including the restored link case, math and
Mermaid interaction, rich clipboard, selection and weather cases. Logs:
`/tmp/decodex-markdown-link-before.log` and
`/tmp/decodex-markdown-reconciled.log`. Strict all-feature/all-target GPUI lint is
recorded in `/tmp/decodex-markdown-reconciled-clippy.log`.

Math, Mermaid and weather are optional presentation capabilities for the user's
later removal decision. Correct original text, source copying and existing file
links are core interaction requirements. No parser or dependency is added in this
batch. Signed desktop font alignment, long-diagram interaction and whole-service
acceptance remain open.
