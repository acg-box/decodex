# Private-artifact authority decision

## Decision and boundary

<a id="rule-PA-DEC-0001"></a>
**[rule:PA-DEC-0001]** Use this fixed package as the single cumulative authority
for the private-artifact subsystem. The package replaces amendment-stack
interpretation with current semantic modules, one current-rule ledger, one
source-ordered census of accepted product semantics, fixed inventories, immutable
V22 bindings, and a raw-byte manifest. The package adds no runtime component,
dependency, schema language, generator, compiler, checker, command, alias, or test
framework.

The package owns only these files:

- `README.md`
- `decision.md`
- `foundations.md`
- `model-codec-reducer.md`
- `persistence-gc.md`
- `executor-platform.md`
- `operations-delivery.md`
- `authority/rules.tsv`
- `authority/source-census.tsv`
- `authority/inventories.json`
- `authority/v22-baseline.tsv`
- `authority/v22-relations.tsv`
- `authority/v22-function-contracts.rs.txt`
- `authority/v22-runtime-execute-functions.rs.txt`
- `authority/package.manifest`
- `corpus/index.tsv`

`corpus/index.tsv` is nonnormative. All other package members have the roles that
`authority/package.manifest` records. The manifest does not include itself.

## Authority and precedence

<a id="rule-PA-DEC-0002"></a>
**[rule:PA-DEC-0002]** Apply package authority in this order:

1. `authority/rules.tsv` identifies the only owner of each current rule.
2. The owner locator contains the normative rule text or the fixed exact value.
3. `authority/inventories.json` owns exact closed sets that prose references.
4. The V22 snapshots and bindings own immutable baseline values.
5. `authority/source-census.tsv` proves the disposition of accepted product-semantic
   source units.
6. `authority/package.manifest` proves member identity, not semantic correctness.

The accepted product corpus is cumulative in the order V3, V4, V4.1, V4.2, and
V4.3. A later source changes an earlier source only where it explicitly amends,
replaces, or retires that meaning. Silence does not delete an earlier rule.
Focused acceptance is evidence of acceptance and is not product-semantic corpus.

`PA-CLOSE-0001` accepts the signed C2 baseline by explicit policy. This package
incorporates the AR-CLOSE amendment directly instead of creating an overlay.
Projections are navigation only. A projection must not contain a package rule marker
or duplicate an exact package inventory.

## Traceability domains

<a id="rule-PA-DEC-0003"></a>
**[rule:PA-DEC-0003]** Every row in `authority/rules.tsv` has exactly one
`origin_class` and one `origin_ref`.

- `corpus` is only for product semantics from accepted V3, V4, V4.1, V4.2, and
  V4.3. Its `origin_ref` is one source ID in `authority/source-census.tsv`.
- `package_native` is only for package governance, authority cutover, quarantine,
  ownership, delivery sequencing, validation policy, and the accepted V2.3
  preparation-surface corrections. Its `origin_ref` is one stable package-native
  origin from the table below.

No product semantic can use `package_native`. Census completeness applies to all
and only `corpus` rules. Every corpus rule is cited by at least one census row, and
every census row terminates at one or more corpus rules. Every package-native origin
is justified below and is consumed by at least one package-native rule.

<a id="rule-PA-TRACE-0001"></a>
**[rule:PA-TRACE-0001]** `authority/rules.tsv` has columns `rule_id`,
`owner_path`, `owner_locator`, `representation`, `origin_class`, and `origin_ref`.
Rows sort by `rule_id`; each ID and owner locator is unique; each owner path exists
in this package; representation is `markdown`, `json`, `tsv`, or `verbatim`; and
origin class is `corpus` or `package_native`. A corpus reference resolves to one
census source ID. A package-native reference resolves to one closed origin below.
The file contains current rules only.

<a id="rule-PA-TRACE-0002"></a>
**[rule:PA-TRACE-0002]** `authority/source-census.tsv` has columns `source_id`,
`corpus_id`, `ordinal`, `start_byte`, `end_byte`, `source_sha256`, `kind`,
`disposition`, `successor_ids`, and `current_rule_ids`. Corpus order is V3, V4,
V4.1, V4.2, V4.3. Offsets are zero-based with exclusive end. Hashes cover exact
unit bytes. Kinds are `paragraph`, `list_item`, `table_row`, and `code_block`.
Dispositions are `retained`, `amended`, and `retired`. Multi-value fields use
comma-separated unique IDs without spaces; `-` means empty.

One contiguous prose paragraph is one unit. One list item, including continuation
lines, is one unit; nested items are separate. Each semantic table header or data
row is one unit; separator rows are not. One complete fenced block is one unit. A
heading is a unit only when it states a product rule. Blank and formatting-only
bytes are not units.

A retained unit has no successor and maps completely to a current rule. An amended
unit has one or more later successors whose closure plus direct mappings preserves
its complete meaning. A retired unit has a later explicit retirement/prohibition
owner. Successor edges are later-only and acyclic. The census contains all and only
accepted product-semantic units. Package governance, research metadata, evidence
binding, owner/delivery policy, and focused acceptance have no census row.

## Stable package-native origins

The following origins are closed. A package change must not add an origin without an
explicit Manager scope decision.

| Origin | Scope | Justification | Required consumers |
| --- | --- | --- | --- |
| `PKG-NATIVE-001` | Package boundary, internal precedence, and no-new-mechanism limit | XY-1374 requires one fixed self-contained package without another framework | `PA-DEC-0001`, `PA-DEC-0002` |
| `PKG-NATIVE-002` | Rule ownership and the disjoint corpus/package-native traceability domains | Manager traceability reset after fresh skeptic review | `PA-DEC-0003`, `PA-TRACE-0001`, `PA-TRACE-0002` |
| `PKG-NATIVE-003` | Raw-byte fingerprints, manifest form, and candidate invalidation | Review must reproduce identity with ordinary read-only tools | `PA-DEC-0004`, `PA-DEL-0003` |
| `PKG-NATIVE-004` | AR-PKG, AR-CUT, and AR-CLOSE ordering | Projections cannot cut over before package freeze, and downstream work needs one accepted public authority identity | `PA-DEL-0001` |
| `PKG-NATIVE-005` | A0/A1/B/D0a/C/D ownership, pre-CORE-FREEZE execution prohibition, D0a exception, and CORE-FREEZE | Moving source needs disjoint ownership and one exact frozen execution boundary | `PA-OWN-0001`, `PA-OWN-0002`, `PA-FREEZE-0001` |
| `PKG-NATIVE-006` | ACC maximum allowlist, minimum acceptance source, mechanical preparation, and bounded aggregate repair | Validation source starts only from one frozen integrated core | `PA-ACC-0001`, `PA-ACC-0002`, `PA-VAL-0001`, `PA-VAL-0002` |
| `PKG-NATIVE-007` | Fixed production SQL locator, distinct canonical tasks, V1-V23 context, and version-2 semantic receipts | Accepted V2.3 correction removes the incomplete five-source preparation contract | `PA-PREP-0001`, `PA-PREP-0002`, `PA-PREP-0003` |
| `PKG-NATIVE-008` | Surface-aware retirement and historical-evidence classification | Fresh skeptic review showed that a global token-zero rule conflicts with a canonical retirement record | `PA-RET-0001`, `PA-RET-0002`, `PA-RET-0003` |
| `PKG-NATIVE-009` | Privacy, rejected-candidate isolation, and package process risks | The accepted package must be self-contained without committing private provenance or rejected prose | `PA-DEC-0005` |
| `PKG-NATIVE-010` | Signed C2 cutover, historical-corpus quarantine, accepted semantic-fidelity residual, external-authority preservation, and zero private-review channel components | Explicit AR-CLOSE risk acceptance replaces the package-native historical semantic-rereview prerequisite | `PA-CLOSE-0001`, `PA-CLOSE-0002`, `PA-CLOSE-0003` |

## C2 cutover and historical provenance

<a id="rule-PA-CLOSE-0001"></a>
**[rule:PA-CLOSE-0001]** Accept signed C2 commit
`019f58a31b976056c000b73de3ec46b89284c6eb`, tree
`a56976663774b1e901e27fdf4c5276a7e9c84cb8`, package subtree
`4e72b4371f09fc96db9b238ba85cd0841694b46e`, and raw
`authority/package.manifest` SHA-256
`8ae497fdc66c373a9a3582f3bf04b81fa4a02f5f8eddd300cbdb76b16662c5a7`
as the Decodex vNext private-artifact cutover baseline by explicit policy. These
identities and public consistency checks prove the selected bytes and bindings only.
They do not prove historical semantic fidelity.

C2 can contain an unknown historical omission, mistranscription, or source-census
mapping error. This is an accepted residual. AR-CLOSE changes package-native
governance only. It does not change `authority/source-census.tsv`, a `corpus` rule,
a product-semantic owner, a fixed V22 binding, or any corpus-derived product
semantic. Reconsider the quarantined source meaning only if public evidence shows
that the bound identity is wrong, a material public contradiction cannot be resolved
without that meaning, a new historical-import requirement makes it indispensable,
an implementation-blocking ambiguity cannot be resolved from public authority, or a
legal requirement or explicit user authority requires historical access.

<a id="rule-PA-CLOSE-0002"></a>
**[rule:PA-CLOSE-0002]** Quarantine the historical private corpus in place as
non-runtime provenance. No Decodex runtime, tool, task, validation, indexer,
automatic discovery, or normal workflow has authority or a configured edge to
read, copy, migrate, index, discover, or consume it. Do not delete, relocate,
summarize, or reinterpret it. Future access requires explicit user authority and
one decision-changing reason from `PA-CLOSE-0001`. Quarantine removes the data-flow
edge; it does not claim secure deletion or hostile-process containment.

<a id="rule-PA-CLOSE-0003"></a>
**[rule:PA-CLOSE-0003]** AR-CLOSE does not amend the external vNext state or
RoleProfile authority. vNext starts with empty PostgreSQL product state, imports no
historical Codex, SQLite, or Linear runtime state, and owns only new
Decodex-created work. The user exclusively controls the four global RoleProfiles.
Each RuntimeSession retains the exact selected model, reasoning effort, service
tier, and instruction revision and digest. No package, channel, or runtime-local
default can substitute for that selection.

The private-review channel component count is zero. There is no channel owner,
blocking rereview gate, caller, binary, crate, schema, service, test suite, or
helper, and this amendment adds no runtime or normal-workflow edge. The vNext
authority contract remains the owner of the state, RoleProfile, and RuntimeSession
product semantics; this package-native rule prevents AR-CLOSE from weakening them.

## Change and review control

<a id="rule-PA-DEC-0004"></a>
**[rule:PA-DEC-0004]** AR-PKG froze one component identity from the raw bytes of
this exact file set, and AR-CUT bound that identity. AR-CLOSE replaces the
package-native cutover prerequisite and regenerates the package and affected
projection identities in one public authority candidate. Any later content change
increments the component candidate identity and invalidates dependent public review
evidence. Counts, clean status, and hashes are evidence of identity only. They do
not prove semantic fidelity.

Future authority changes must update each affected semantic owner, rule-ledger row,
corpus disposition, exact inventory, source binding, manifest row, and projection
binding in one candidate. A governance-only change leaves unaffected corpus and
product-semantic files byte-identical but still regenerates every affected package
identity. Do not create another overlay amendment as current authority.

## Privacy, rejected evidence, and process risks

<a id="rule-PA-DEC-0005"></a>
**[rule:PA-DEC-0005]** Do not commit raw session payloads, private paths, session
identifiers, task identifiers, or timestamps. `corpus/index.tsv` can contain only
public corpus labels and fingerprints. The rejected seven-file candidate is
navigation evidence only. Do not copy, repair, apply, or use its wording as the sole
source of a rule.

The package process risks are separate from product residual risks:

| ID | Risk | Control |
| --- | --- | --- |
| `PKG-PROCESS-01` | A source unit can be classified incorrectly. | Accept the residual in `PA-CLOSE-0001`; reconsider only on one listed decision-changing condition. |
| `PKG-PROCESS-02` | A source unit can map to incomplete current semantics. | Accept the residual in `PA-CLOSE-0001`; a public implementation-blocking contradiction stops the affected work. |
| `PKG-PROCESS-03` | Quarantined private provenance can become unavailable. | Keep the public package self-contained; do not copy or otherwise act on the quarantined provenance. |
| `PKG-PROCESS-04` | A projection can drift without a rule marker. | Inspect every current public projection during each authority change and update each affected binding. |
| `PKG-PROCESS-05` | A package change can invalidate dependent evidence. | Recompute package and projection identities and restart only the affected public review. |
| `PKG-PROCESS-06` | Mechanical integrity can pass while semantics are wrong. | State the accepted semantic-fidelity residual; mechanical agreement cannot close it. |
