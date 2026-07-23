# Private-artifact authority package

This directory is the accepted cumulative authority for the Decodex private-artifact
subsystem. The AR-CLOSE governance amendment accepts signed C2 commit
`019f58a31b976056c000b73de3ec46b89284c6eb` and tree
`a56976663774b1e901e27fdf4c5276a7e9c84cb8` as the cutover baseline by explicit
policy. Package identity does not prove historical semantic fidelity.

Start with [decision.md](decision.md). Then read the semantic modules in this order:

1. [foundations.md](foundations.md)
2. [model-codec-reducer.md](model-codec-reducer.md)
3. [persistence-gc.md](persistence-gc.md)
4. [executor-platform.md](executor-platform.md)
5. [operations-delivery.md](operations-delivery.md)

The `authority/` directory contains the current-rule ledger, the accepted-corpus
census, fixed inventories, and immutable V22 source bindings. The optional
`corpus/index.tsv` contains fingerprints only. No private session path, session
identifier, task identifier, timestamp, or payload is part of the package.

AR-CLOSE changes package-native governance and affected projections only. It leaves
the source census, corpus-derived product semantics, product source, executable
behavior, and command authority unchanged. Its accepted quarantine, residual-risk,
cutover, and delivery rules are in [decision.md](decision.md) and
[operations-delivery.md](operations-delivery.md).
