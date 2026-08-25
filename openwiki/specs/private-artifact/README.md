---
type: "Reference"
title: "Retired private-artifact design archive"
openwiki_generated: true
---

# Retired private-artifact design archive

Status: XY-1403 Option 1 exact-candidate review is approved for commit
`6f03833a33d82f3d1fff00d45b518e8701fbd582`, tree
`50d531b59bc818bf94f358398d3e8d1b33d26eed`, in Linear comment
`669ea305-3709-4aa3-ae59-df3e68e41cee`. PR landing and Linear closure remain
pending. The [retirement decision](decision.md) defines the exact repository
effective point. Review approval alone does not make the candidate product
authority. That requires authoritative landing and landed commit, tree, and path
readback.

At and after that effective point, Decodex vNext has no private-artifact subsystem,
delivery lane, or future-work promise. The semantic modules and their rule markers are
frozen historical evidence only:

1. [foundations.md](foundations.md)
2. [model-codec-reducer.md](model-codec-reducer.md)
3. [persistence-gc.md](persistence-gc.md)
4. [executor-platform.md](executor-platform.md)
5. [operations-delivery.md](operations-delivery.md)

The protected package data describes the retired design. It is not a current rule
ledger, dependency graph, runtime input, validation input, or future-work inventory.
In particular, `authority/rules.tsv`, `authority/inventories.json`,
`authority/source-census.tsv`, all four `authority/v22-*` files, and
`corpus/index.tsv` remain byte-identical historical evidence. The role labels in
`authority/package.manifest` identify archive members only. They do not restore
authority to those members.

The archive contains no private session path, session identifier, task identifier,
timestamp, payload, or new runtime mechanism. Do not use it to create a service,
schema, storage system, runtime route, platform layer, issue, compatibility path,
command, test, or delivery gate.

The accepted Artifact/BlobStore boundary remains unchanged for ordinary product
evidence. XY-1369, XY-1370, and XY-1363 use the bounded canonical privacy-safe Git
evidence contract in [decision.md](decision.md). They do not require a new product
Artifact or any private-artifact machinery.
