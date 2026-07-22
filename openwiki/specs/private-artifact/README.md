# Private-artifact authority package

This directory is the XY-1374 authority candidate for the Decodex private-artifact
subsystem. It is bound to Git commit
`4daf4dd809411bc83d7ea912e6b99612d4c9572a`.

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

The package does not change product source, executable behavior, command authority,
or any projection. AR-CUT owns projection changes after this package is frozen.
AR-REV owns independent review of the stacked package and projection candidate.

