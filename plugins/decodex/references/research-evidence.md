# Decodex Research Evidence

Use this reference for evidence collection, classification, and traceability.

## Rule

No evidence, no claim.

Separate:

- observations
- contradictions
- inferences
- gaps
- source refs
- code refs
- live readback
- private proof
- public-safe provenance

## Evidence Classes

| Class | Use |
| --- | --- |
| `external_source` | Public specs, official docs, standards, changelogs, vendor policy. |
| `repo_source` | Checked-in files, code refs, fixtures, tests, command output. |
| `live_readback` | Current runtime, tracker, GitHub, Linear, local DB, or service state. |
| `inference` | A reasoned conclusion derived from named evidence. |
| `gap` | Missing proof, unresolved contradiction, blocker, or human choice. |

## Ledger Requirements

A later agent must be able to answer:

- Which claims are externally supported?
- Which claims are supported by repository state?
- Which claims come from live readback?
- Which conclusions are inferences?
- Which gaps remain, and do they block `decision_ready`?

Preserve conflicting evidence. Do not flatten contradictions into confidence.

