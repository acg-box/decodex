# Lane Authority V2 C1I Inventory Contract

Status: normative P0 contract for XY-1251. `C1I_INCOMPLETE`.

This contract defines the closed-world inventory that must exist before C1A changes a
launcher or C1B introduces dormant Lane Authority v2 runtime foundations. It refines the
C1I gate in [Lane Authority v2 gate manifest](lane-authority-v2-gates.md) and the source
inventory requirements in
[Lane Authority v2 effect registry](lane-authority-v2-effects.md). P0 changes only this
contract, schemas, and checkpoint evidence. It does not classify the repository, change
runtime behavior, or authorize C1A/C1B.

The machine-readable P0 contract is split by authority rather than stored as one
self-approving document:

- `tools/lane-authority-inventory/contracts/analysis_cut.schema.json` defines the
  non-self-referential accepted source cut;
- `tools/lane-authority-inventory/contracts/authority_surface_catalog.schema.json` and
  `tools/lane-authority-inventory/catalog/authority_surface_catalog.json` define the
  independent catalog input, which remains intentionally empty in P0;
- `tools/lane-authority-inventory/contracts/dataflow_contract.schema.json` and
  `dataflow_contract.json` freeze the finite lattice, transfer rules, limits, Top reasons,
  and proof receipt;
- `tools/lane-authority-inventory/contracts/inventory_composition.schema.json` separates
  candidates, many-to-many edges, adjudications, and orthogonal site classifications;
- `tools/lane-authority-inventory/contracts/rejection_report.schema.json` defines the
  deterministic sanitized negative evidence; and
- `tools/lane-authority-inventory/contracts/p0_checkpoint.schema.json` and
  `p0_checkpoint.json` bind the provisional base, C0 artifacts, candidate anchors,
  migration state, plan review, and literal `C1I_INCOMPLETE` advancement state.

The P0 checkpoint field is named `provisional_analysis_cut_anchor`; it is not an instance
of the P1 `analysis_cut.schema.json` and cannot substitute placeholder/null values for a
complete `C1IAnalysisCut`. P1 creates the first full analysis-cut instance only after all
required source/delta/tool/supporting-input digests and counts are measured.

`scripts/verify_lane_authority_v2_c1i_contract.sh` validates this P0 structure. The
normative readiness command `scripts/verify_lane_authority_v2_gates.sh C1I` must return a
reason-coded rejection until P5; a zero exit before then is itself a contract failure.

## Safety Claim And Stop Rule

C1I may report ready only when one accepted composition proves all of the following for
the exact landing source cut:

- every in-scope source byte has a parsed source node and scope/config projection;
- every C0 candidate category has an explicit candidate adjudication;
- every authority-relevant syntax, symbol, data, call, declarative, and generated site has
  one exact classification and owner/disposition;
- every local authority edge reaches a reviewed boundary, and every external symbol is a
  cataloged authority root or reviewed non-authority symbol;
- every dynamic target has a finite proof, with no unresolved/top value or missing edge;
- every accepted manifest is deterministic and contains no unresolved state; and
- the exact PR base/head readback still matches the approved analysis cut.

Any missing source, parser recovery node, unknown scope/config, unresolved symbol/edge,
dynamic top value, catalog mismatch, changed PR base, or unreviewed semantic digest is a
hard rejection. The gate must emit a rejection report; it must never weaken a rule or
silently omit a site to obtain zero unresolved findings.

## Exact Analysis Cut

The C0 observations remain immutable and are not regenerated. Their source anchor is:

```text
baseline commit: d57553bc1bcdceebe1d0c7ec5ad5dc492b695348
source file count: 3,363
source tree digest: d55e72c9b4a522f7cec8af4afa8c968c6fd3749139d045d178160b6287f00507
launcher artifact: f7d104ba81a793073654082abb6fbda5695ad916b2c5082dd00a67c15d9ad8c9
legacy artifact: 7443fb30ccbeefe9240d36074de7ec51a29c9b4cd3a378933628762012434917
mutation artifact: d0cbd97dfe32376d8a1d41a905a7b85bb5b4eee5c77b1cd2c13a219902fdfee8
scenario artifact: c87acaa1373c4a4bc45833e116c9d78208be932d8690fb78108c83d5ffabb914
```

C1I analyzes more than that frozen tree. A `C1IAnalysisCut` contains:

```text
repository_key = github.com/hack-ink/decodex
c0_baseline_commit
c0_source_tree_digest
c0_artifact_sha256[4]
pr_base_commit
pr_base_tree
analysis_input_tree_digest
analysis_source_node_count
analysis_source_nodes_digest
tool_source_nodes_digest
```

The analysis cut binds only immutable Git/source identity. It does not bind parser,
supporting-input, cfg, toolchain, catalog, or dataflow outputs that do not exist until
P2/P3. Those outputs are bound exactly once by the accepted composition. This separation
prevents a later-phase artifact from becoming a placeholder or circular prerequisite for
the P1 source cut.

`analysis_input_tree_digest` is a domain-separated digest over every tracked in-scope
source byte at the exact C1I source cut. Generated accepted manifests and checkpoint
evidence are outputs and are excluded from their own preimage. The final gate recomputes
the input digest from the exact PR head and proves that any commits after the source cut
change only allowlisted output/evidence paths and leave every analyzed source byte
unchanged.

Excluding an output from its own input preimage is not an authority exclusion. Every
output has its own artifact digest, the composition binds that digest exactly once, the
checkpoint ledger binds the artifact set, and the exact-head gate verifies both the input
digest and output artifact digests. The machine policy is
`outputs_excluded_from_input_preimage_and_bound_by_artifact_digest_ledger_and_exact_head_gate`;
no output may disappear from those latter bindings.

`output_artifact_policy.json` is the closed machine-readable output universe. It lists the
analysis cut, composition, every typed relation manifest, cfg coverage, dataflow proofs,
tool/review receipts, rejection report, and ledger with an explicit binding mode. The
analysis cut binds the policy digest instead of embedding a partial hand-maintained path
list. An output absent from the policy or a relation absent from the composition is a
contract failure.

The P1 `source_inventory.json` contains the identity projection shared by later enriched
source records: path, byte length/content digest, language, scope, provenance, status,
predecessor, and canonical source id. Its partition digests deliberately exclude later
parser receipts and syntax counts, so P2 can add analysis evidence without changing the
immutable source cut.

The materializer reads Git objects, not mutable worktree bytes. It creates a temporary
read-only tree and verifies every path/content digest before parser or compiler startup.
Cargo, rust-analyzer, SwiftPM, SourceKit, Python, shell, TOML, and YAML analysis run in
that tree with caches and outputs outside it.

### Post-C0 Delta Closure

The effective source universe is the union of:

1. every C0 source node;
2. every in-scope source added or modified between C0 and the exact PR base;
3. every in-scope source added or modified by C1I, including inventory tool code; and
4. tombstones for deleted C0/delta nodes, preserving their candidate and replacement
   disposition.

Modified nodes retain predecessor content identity and receive a new current identity.
Deleted nodes never disappear from candidate accounting. Upstream main changes are not
C1I runtime edits, but they are mandatory inventory input. The changed-path gate compares
the PR diff to its exact PR base, while the inventory gate compares the full analysis
source universe to C0.

Before ready and again immediately before land, the gate reads the PR and canonical main
through explicit `github.com/hack-ink/decodex` authority. A changed PR base, a head that
does not contain that base, or a changed analysis input digest invalidates accepted
manifests, CI, and every prior ready review. Rebase, regeneration, validation, and a fresh
skeptic review are then mandatory.

The P0 branch was most recently fast-forwarded to provisional base
`51f553fd32c8f75eed925afe87f99931844fffec`. This is evidence, not a permanent landing
base; the final readback rule above remains authoritative.

## Normative Authority Surface Catalog

`tools/lane-authority-inventory/catalog/authority_surface_catalog.json` is a reviewed,
versioned semantic input. Its signatures, capability classes, authority relevance,
ownership, owners, replacements, removal checkpoints, and reason codes are never
generated from C0 regex candidates or scanner output. P3 may materialize only exact
consumer ids, used-site digests, and dispositions for those independently supplied
semantic entries.
`tools/lane-authority-inventory/catalog/external_symbol_policy.json` is the independent,
signature-exact semantic input for proposed non-authority external symbols. P3 may bind
an enumerated source consumer only to the same `(language, signature)` policy identity;
it must not infer, widen, or generate a policy decision. The policy schema intentionally
has no filesystem, process, environment, SQL, network, provider, time, or other
authority-capable class. Unlisted and dynamic symbols remain unresolved until separately
adjudicated. The policy remains machine-validated and review-pending until the P5 review
approves its exact semantic digest with the complete populated catalog.

`tools/lane-authority-inventory/catalog/authority_symbol_policy.json` independently
enumerates signature-exact authority roots with capability class, semantic kinds, owner,
current ownership, replacement, removal checkpoint, and reason. Scanner output and C0
candidates may discover proposed entries but cannot create or widen them. Directly
qualified standard-library roots may be admitted before receiver typing; variable- or
field-qualified calls such as database, provider, process, and file handles remain
unresolved until their receiver type is proven. Every admitted authority entry must have
at least one exact consumer, and every consumer remains bound by a catalog disposition.

Rust receiver proof is conservative and explicit. P3 may canonicalize an object method
to `ImportedType::method` only when tree-sitter proves an explicit parameter type, an
explicit local binding type, or a same-file enclosing struct field type and resolves the
type through a unique structured `use` path. The symbol relation records the canonical
receiver type and evidence kind. Inferred return types, ambiguous imports, untyped
bindings, arbitrary method chains, and cross-file field shapes remain unresolved.

It has closed, language-qualified sections for:

- canonical external symbol/API signatures used by the analysis source universe;
- mutation, authority-read, authority-discovery, and launcher roots;
- SQL tables/columns/statements and persistent record roots;
- provider fields and semantic configuration/environment/file/path roots;
- executable declarative YAML/TOML key paths and their runtime consumers;
- generic HTTP, process, filesystem, SQL, FFI, reflection, eval, shell source,
  `xargs`, `find -exec`, and `sh -c` capability roots;
- local wrapper/adapter closure boundaries, effect owner, replacement kind, and removal
  checkpoint; and
- the supported target/config/toolchain matrix and reviewed non-authority external
  symbols.

Every resolved external symbol used by an analyzed source node must have one exact
catalog disposition. Package/module wildcards are forbidden unless the catalog also
contains the complete used-symbol set and its digest, so adding a new external symbol
cannot inherit an old non-authority decision silently. Generic capability roots are
always authority-relevant until finite dataflow proves a narrower registered operation.

The scanner enumerates source sites independently, then joins them to the catalog.
Starting from catalog roots, it reverse-closes all resolved local call and dataflow edges
to reviewed adapters/boundaries. It is invalid to use the C0 regex set as sink authority
or to classify only sites already named by the catalog.

The catalog has a domain-separated semantic digest. P0 schema validation does not approve
P3 contents. P0-P4 use machine validation only; the populated catalog is reviewed as part
of the complete P5 exact-head input. Any later semantic digest change invalidates that
integrated ready review.

## Complete Site Universe

Each supported parser enumerates all syntax nodes before authority classification. The
inventory includes, at minimum:

- calls, method calls, constructors, macros, closures, callbacks, command builders, and
  declarative executable nodes;
- declarations, imports, bindings, assignments, field/key/index reads and writes,
  pattern destructuring, matches, conversions, and return/parameter flow;
- serialization/deserialization, environment/configuration/path access, provider object
  construction, SQL statements/identifiers/bindings, literals, interpolation, and
  argument assembly; and
- include/macro/build/generated inputs and their resulting parsed sites.

Every source parser traverses every named node and publishes an exact total node count and
domain-separated digest over `(kind, byte_start, byte_end)`. The persisted syntax relation
materializes parser roots, candidate-covering nodes, and authority-relevant executable
nodes; completeness comes from the total traversal receipt, not from serializing every
identifier or literal into redundant JSON. Every catalog data root must map to one or more
materialized syntax/data sites or to a reviewed absent receipt. A
catalog root with neither is a rejection, not evidence that the root is unused.

The accepted composition consists of these separately versioned relations:

```text
source_nodes
cfg_projections
syntax_sites
symbol_sites
data_sites
call_edges
dataflow_edges
catalog_entry_dispositions
candidate_records
candidate_site_edges
candidate_adjudications
site_classifications
supporting_inputs
toolchain_receipts
```

The sole effective C1I inventory is the composition of those relations, the four bound
C0 artifacts, and the catalog/supporting-input/config/toolchain digests. No C0 manifest
is rewritten and no second effective reader is permitted.

Every relation is a closed typed manifest, not an opaque count/digest receipt. The
composition binds its exact path, schema, count, and byte digest. The normative verifier
validates every record against Draft 2020-12 JSON Schema, then executes cross-relation
proofs over the loaded records. Declaring an invariant name without executing it is not
evidence.

Every catalog entry, not only external symbols, has either exact matched-site
dispositions or one reviewed-absent receipt. The verifier checks entry-kind/site-kind,
source language, consumer ids, and the domain-separated used-site digest. External
symbols additionally bind exact signature digests.

The source relation is partitioned, not merely counted in aggregate. `analysis` nodes are
current, non-tool nodes with `c0`, `post_c0_base`, or `c1i_head` provenance; `tool` nodes
are current nodes with tool provenance and scope; and `deleted_tombstone` nodes are deleted,
non-tool nodes with a predecessor. Each partition must equal its corresponding exact
analysis-cut count and canonical partition digest. The digest binds every sorted source-node
record field, so provenance or scope cannot be relabeled within the same count partition.
This nonzero equality prevents an empty or relabeled set from satisfying totality statements
vacuously.

Each source node also binds its exact byte length, a parser receipt, exact syntax-site
count, and a domain-separated digest of its syntax-site id set. Every current source
publishes exactly one parser root spanning byte range `0..byte_length`, including an empty
file; only a tombstone has zero syntax and the fixed deleted disposition. Candidate,
call-edge, and dataflow relations must be
nonempty for this repository, so absence cannot masquerade as complete analysis.

## Candidate And Site Classification

The replay adapter imports the checked-in C0 pattern definitions without changing the C0
generator. Against verified C0 bytes it reconstructs one `candidate_record` per
`(source_node, category, line identity)` and proves every C0 count, first-line value,
candidate digest, source identity, and artifact SHA unchanged.

Each replayed candidate records its launcher, legacy, and/or mutation artifact origins.
Origin membership is unique per record, and the verifier recomputes exact per-artifact
counts against the frozen C0 anchors. Post-C0 candidates have no C0 origin membership.
The verifier also reconstructs each immutable C0 observation and checks its source path,
category, line count, first line, and original SHA over ordered line-number/line-digest
pairs. Candidate ids or origin labels cannot substitute for that replay.

Candidate-to-site mapping is many-to-many. Each candidate category, including two
categories on the same line, has exactly one `candidate_adjudication`:

```text
candidate_id
candidate_category
related_site_ids[]
disposition = covered_by_sites | regex_false_positive |
              declarative_document | supporting_input | deleted_since_c0
evidence_digest
reason_code
review_identity
```

`regex_false_positive` applies only to a C0 observation. It cannot erase a real syntax,
call, or dataflow edge. A source line with several candidates receives one adjudication
per category; no line-level shortcut may apply one result to all categories.
For `covered_by_sites`, `related_site_ids` is nonempty and exactly equals the candidate's
edge target set. Every other disposition requires both sets empty. Candidate category
must equal the referenced candidate record category.

Each syntax/data/call site classification uses orthogonal fields:

```text
scope = production | test | mixed | generated | tool
semantic_kinds[] = mutation | authority_read | authority_discovery |
                   launcher | data_only | declarative_executable
runtime_generation = v12 | v2 | not_runtime
authority_relevance = authority_surface | reviewed_non_authority
ownership = registered_adapter | legacy_direct | not_applicable
owner
replacement_kind
removal_checkpoint
reason_code
target_projection
config_projection
```

Cfg projection records carry `projection_kind = target | config`. A source-root projection
is inherited by materialized descendant sites; a nested conditional may add a narrower
projection. Both classification references must resolve to the classified site's source
and to the matching kind; a globally existing projection for another source or dimension
is not closure evidence.

Accepted manifests contain no `unknown`, unresolved edge, missing owner, or unadjudicated
candidate. `reviewed_non_authority` requires a reason and may not terminate an existing
authority-relevant call/dataflow edge. Declarative executable sites remain connected to
their consumers.

## Configuration And Target Closure

The analyzer derives the complete Rust/Swift conditional-compilation atom universe from
the exact analysis source, rather than assuming Linux/macOS plus test covers every node.
The build/config matrix must include supported Linux/macOS production and test targets,
all features/targets, Swift executable/test targets, and every satisfiable supported
conditional projection.

Every syntax node receives one of:

```text
active_supported
inactive_supported_projection
unsupported_by_product_contract
generated_from_bound_input
```

An inactive or unsupported node is still parsed and conservatively classified. Its local
edges must resolve under a synthetic/compiler projection or remain a rejection. Product
support evidence such as the non-Unix compile error may justify
`unsupported_by_product_contract`; it may not remove the node from source/candidate
accounting. The `cfg_coverage_manifest` proves every node appears in at least one
projection or has that explicit unsupported disposition.

The verifier checks both directions: every cfg projection references an existing syntax
site, and every current source has both config and target projection coverage inherited by
its materialized sites, including an explicit unsupported disposition where applicable.
An empty cfg relation cannot satisfy C1I.
Every site classification's target and config projection ids must resolve to those
projection records. Classification scope equals the underlying source scope, and each cfg
projection records the source language and platform. A toolchain receipt may use only
config projections for its own language and either its exact platform or `common`.

Linux and macOS CI publish nonempty platform-tagged slices and completion receipts. The
union of exact-platform projection ids in each platform's receipts must equal that
platform's complete cfg projection set. Common source/config slices must be
byte-equivalent. The accepted inventory is a deterministic union; an absent platform job,
empty platform projection set, or incomplete slice blocks C1I.

The accepted `cfg_coverage.json` is schema-validated and binds the analysis cut, cfg
relation, full syntax-site set, covered-site set, platform slices, and its self-bound
artifact digest. The composition binds the file SHA.

## Bounded Dataflow Proof

Dynamic targets pass only through the following fixed-point abstract interpretation:

```text
Bottom
Constant(value_digest)
FiniteSet(value_digest[], maximum_cardinality)
Structured(kind, ordered_parts[])
AuthorityRoot(root_ids[])
Top(reason_code)
```

The analyzer propagates parameter/return/field flow across the complete local call graph
and strongly connected components until a deterministic fixed point. Allowed exact
transforms are constant construction, finite enum/match selection, constant-format
interpolation, path join/push over finite values, typed wrapper projection, and reviewed
serialization mappings. Alias uncertainty, unknown reflection, unresolved trait-object
dispatch, unbounded collection/string input, recursion widening, unsupported transform,
or cardinality overflow produces `Top`.

Any path from `Top` to an authority read, mutation, discovery, launcher, provider,
process, filesystem, SQL, FFI, eval, or executable declarative target rejects. A passing
proof records source and sink site ids, call/dataflow edge ids, catalog entries, config
projections, transfer rules, tool receipts, a finite result value, and a fixed-point
digest. The verifier reconstructs the selected directed graph, proves source-to-sink
reachability, rejects every selected edge outside all source-to-sink paths, binds config
projections to nodes on the proven path, and recomputes the proof digest. Increasing
limits or adding a transform changes the semantic analyzer digest and requires
catalog-level skeptic review.

Python `getattr`/`exec`, Swift selectors, Rust trait objects/FFI, shell
`eval`/`source`/`xargs`/`find -exec`/`sh -c`, dynamic SQL, and dynamic provider/process/
filesystem targets have no exception outside this proof.

The accepted `dataflow_proofs.json` binds the analysis cut, transfer contract, call and
dataflow relations, every authority sink, source sites, finite results, referenced
catalog/config/tool evidence and edges, fixed-point digests, and zero Top-reaching sinks. The
composition binds the file SHA.

## Generated And Supporting Inputs

Macro/include/build-script/generated inputs live in a supporting-input manifest with
path/object identity, producer and producer receipt, consumer, content digest,
scope/config projections, materialized source identity, and authority capability. Known
data-only macros may be cataloged. Authority-capable expansions must bind an exact current
source path/digest/scope, be present in the producer's completed source set, have config
projections owned by that source, and have graph paths to every declared consumer. An
unfrozen, nondeterministic, missing, or authority-relevant generated input rejects.

Parser `ERROR`/`MISSING` recovery, incomplete indexing, empty semantic-service output,
or a tool/SDK/grammar identity outside the reviewed allowlist rejects. Index completion
receipts enumerate the expected and completed targets/source nodes; elapsed time or an
idle process alone is not completion evidence.

Toolchain receipts are an exact proof against the approved catalog matrix: language,
platform, tool identity, digest, and config projections must match exactly, all six
supported languages must be covered, every referenced projection must resolve to a config
projection, and duplicate semantic matrix/receipt rows are rejected rather than collapsed.
Each receipt also binds the exact expected and completed source-node sets plus the
syntax, candidate, call-edge, and dataflow-edge output sets by count and
domain-separated id digest. Accepted receipts have zero unresolved items and no rejection
reason codes; every source parser receipt resolves to one of these complete receipts.
For each language, the sole common parser receipt derives its expected source set from the
analysis-cut language partition, independently of source-record receipt assignments.
External-symbol dispositions reference only unique entries from the two external-symbol
catalog sections. Entry language must match the source language, and the site signature
digest is SHA-256 over the UTF-8 exact catalog signature. The entry's `consumer_ids` and
domain-separated `used_site_set_digest` must equal its complete disposition site set.

## Accepted And Rejected Outputs

Accepted manifests are canonical JSON with relative paths, byte ranges, stable ids,
sorted maps/sets, and domain-separated digests. Absolute paths, wall-clock timestamps,
cache locations, traversal order, thread scheduling, and raw provider/secret values are
forbidden from accepted preimages.
All contract JSON is decoded with duplicate-key rejection; last-key-wins parsing is invalid.

Every failed run emits a deterministic sanitized rejection report outside the accepted
manifest set. It includes reason codes, stable site/candidate ids, parser/tool receipts,
counts, and expected/actual digests, but no source snippets, absolute paths, secrets, raw
provider ids, or private runtime state. CI retains the report as an artifact; the
checkpoint ledger records its digest, reason/count summary, phase commit, and CI run.
Raw failed-run output is diagnostic evidence and never becomes authority.

Generation under sorted, reversed, seeded-shuffled traversal and different supported
parallelism must produce byte-identical accepted manifests and normalized rejection
reports.

Review cadence is checkpoint-based, not edit-based. The C0 architecture review is the
only pre-implementation review. P0-P4 use deterministic machine validation and remain
`C1I_INCOMPLETE`; they do not request independent implementation approval. P5 uses one
non-self-referential integrated-ready receipt covering every changed and untracked C1I
path except the receipt itself, with exact path/byte digests and base commit/tree. A later
byte or base change invalidates that receipt mechanically. The complete Lane Authority v2
change receives a separate exact-head review at C7 before land.

## Tool And Runtime Boundary

The inventory implementation lives under `tools/lane-authority-inventory/` as a
standalone Rust workspace with an empty `[workspace]` and its own `Cargo.lock`. A Swift
helper has a checked-in `Package.resolved`; Python helpers use the reviewed interpreter
allowlist. Root runtime Cargo/Swift manifests, root lockfiles, runtime sources, and the
runtime dependency graph must be byte-identical to the exact PR base.

C1I may change only inventory tools/parsers/catalogs, verifier scripts, C1I fixtures,
dedicated CI jobs, and Lane Authority v2 docs/evidence. A production runtime diff fails
even if tests pass. Inventory tools are classified with `scope=tool` and may not mutate
the repository, runtime database, tracker, provider, worktree, refs, or external state.

New C1I-only fixtures live under `tools/lane-authority-inventory/fixtures/`. The four C0
fixtures under `apps/decodex/src/**/fixtures/lane_authority_v2/` remain immutable inputs;
P1-P5 may bind or replay them but may not edit them. This keeps the changed-path gate from
treating an app fixture directory as a blanket runtime-source exception.

## Auditable P0-P5 Slices

All slices land in one C1I PR, but no intermediate slice is ready:

| Phase | Required output | Advancement state |
| --- | --- | --- |
| P0 | This contract, catalog/composition schemas, rejection taxonomy, negative readiness fixture, current-base drift record | `C1I_INCOMPLETE`; readiness must fail |
| P1 | Git-object materializer, immutable source-identity inventory, exact analysis cut, post-C0 delta/tombstones, C0 candidate replay, four artifact/anchor proofs | `C1I_INCOMPLETE`; readiness must fail |
| P2 | All language parsers, complete site universe, cfg/source/call/dataflow graph, supporting-input/tool receipts | `C1I_INCOMPLETE`; rejection report records nonzero unresolved |
| P3 | Populated catalog, external-symbol closure, candidate adjudications, site classifications, finite dataflow proofs | unresolved reaches zero under machine validation; not ready |
| P4 | Deterministic accepted manifests, five normative scripts, Linux/macOS CI, positive/negative fixtures, runtime-byte/dependency proof | technically complete but not ready or landed |
| P5 | Fresh canonical-main/PR-base readback, regeneration if changed, all C1I commands, `cargo make check`, exact-head CI, fresh final skeptic and code review, Decodex land/readback | only phase that may report ready/landed |

Each phase checkpoint records exact commit, PR base, analysis input/catalog/tool digests,
source/site/candidate/edge counts, unresolved reason counts, validation commands and exit
codes, objections/dispositions, scope changes, migration state (`not started` for C1I),
and the next phase. A nonfinal checkpoint must contain literal `C1I_INCOMPLETE` and the
readiness verifier must reject it.

## C1I Exit Evidence

C1I exits only when:

- all five commands in the C1I gate manifest pass on the exact approved source cut;
- all accepted relation manifests compose exactly once and have zero unresolved state;
- every C0 and post-C0 candidate/site is classified under the current catalog;
- Linux/macOS slices and deterministic reruns agree;
- root runtime bytes/dependencies are unchanged from the exact PR base;
- exact-head CI and fresh skeptic/code review report no blocker, high, or medium gap;
- Decodex-owned commit/PR/land/readback succeeds; and
- the ledger records the landed merge and keeps C1A blocked until C1I is deployed and
  its inventory identities are available to the next gate.

Until then, C1A and C1B remain prohibited.
