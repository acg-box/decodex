# Lane Authority v2 Inventory Tool

Status: `C1I_INCOMPLETE` at P2.

This directory owns the tool-only contracts for the C1I closed-world inventory. It is
not part of the root Cargo workspace and no Decodex runtime crate may depend on it.

P0 froze schemas, reason codes, the bounded dataflow contract, and a negative readiness
fixture. P1 materializes an immutable Git/source identity cut and replays all frozen C0
candidates. P2 traverses every source with pinned tree-sitter grammars, persists per-source
node count/digests, and materializes roots, candidate sites, executable sites, source-level
cfg projections, and candidate edges. It remains unresolved and cannot authorize C1A or
C1B.

The Python verifier uses the tool-local dependency declared in `requirements.txt` and
fully resolved with hashes in `requirements.lock`; it does not modify the root runtime
dependency graph.

Validate the current incomplete phase:

```sh
scripts/verify_lane_authority_v2_c1i_contract.sh
tools/lane-authority-inventory/run_locked_python.sh \
  -m unittest tests.scripts.test_lane_authority_v2_c1i_contract
```

After committing all P1 source/tool changes, regenerate from that immutable commit:

```sh
tools/lane-authority-inventory/run_locked_python.sh \
  tools/lane-authority-inventory/materialize_p1.py --source-cut HEAD
scripts/verify_lane_authority_v2_c1i_contract.sh
```

P2 regeneration uses only the P1 Git cut and a temporary read-only archive:

```sh
tools/lane-authority-inventory/run_locked_python.sh \
  tools/lane-authority-inventory/materialize_p2.py
```

P3 first projects only independently approved exact external-symbol policy identities;
unlisted or dynamic symbols remain unresolved:

```sh
tools/lane-authority-inventory/run_locked_python.sh \
  tools/lane-authority-inventory/materialize_p3.py
tools/lane-authority-inventory/run_locked_python.sh \
  tools/lane-authority-inventory/verify_contract.py --phase P3
```

P0-P4 require machine validation only and remain incomplete. At P5, the preimage command
validates the integrated exact-head input and emits the digest for the single C1I ready
review. Its receipt is excluded from its own digest and cannot approve a different base
or byte set.

The readiness probe must reject:

```sh
scripts/verify_lane_authority_v2_gates.sh C1I
```

Expected result: exit 1 with reason `c1i_phase_incomplete`. A zero exit before P5 is a gate
failure.

Normative design authority lives in
`openwiki/specs/lane-authority-v2-inventory.md`. C0 manifests remain immutable.
