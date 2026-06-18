---
name: research-decision
description: Use when research needs terminal status.
---

# Decodex Research Decision

End every bounded research run with one terminal status. Read
`../../references/research-contract.md` for outcome gates and contract shape.

- `decision_ready`: safe for post-promotion shaping.
- `not_decision_ready`: useful evidence, unsafe decision.
- `blocked`: non-decision blocker remains.
- `needs_human_decision`: remaining uncertainty is human/product/authority choice.
- Include the promotion target and evidence ledger summary in the terminal contract.
- Do not use multiple statuses, choose readiness because budget ended, promote here,
  or write a new Decodex run as a `docs/research/` event log or JSON.
- If the research is persisted under `docs/research/`, persist it as a Markdown OKF
  research concept, never as JSON.
