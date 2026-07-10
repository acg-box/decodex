# Lane Authority v2 Inventory Tool

Status: `C1I_INCOMPLETE` at P0.

This directory owns the tool-only contracts for the C1I closed-world inventory. It is
not part of the root Cargo workspace and no Decodex runtime crate may depend on it.

P0 freezes schemas, reason codes, the bounded dataflow contract, and a negative readiness
fixture. It does not contain accepted source/candidate/site manifests and cannot authorize
C1A or C1B.

The P0 Python verifier uses the tool-local dependency declared in `requirements.txt` and
fully resolved with hashes in `requirements.lock`; it does not modify the root runtime
dependency graph.

Validate P0 machine contracts:

```sh
scripts/verify_lane_authority_v2_c1i_contract.sh
tools/lane-authority-inventory/run_locked_python.sh \
  -m unittest tests.scripts.test_lane_authority_v2_c1i_contract
```

P0-P4 require machine validation only and remain incomplete. At P5, the preimage command
validates the integrated exact-head input and emits the digest for the single C1I ready
review. Its receipt is excluded from its own digest and cannot approve a different base
or byte set.

The readiness probe must reject:

```sh
scripts/verify_lane_authority_v2_gates.sh C1I
```

Expected result: exit 1 with reason `c1i_phase_incomplete`. A zero exit at P0 is a gate
failure.

Normative design authority lives in
`openwiki/specs/lane-authority-v2-inventory.md`. C0 manifests remain immutable.
