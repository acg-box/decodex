---
name: pro-research
description: Consult ChatGPT Pro for a difficult research or design question when the user requests Pro or this Agent has permission to delegate research to its connected ChatGPT subscription.
---

# Pro research

Use the Decodex Research tools for questions that benefit from an independent, deeper analysis. Keep ordinary execution in this Agent. Send only the task, relevant evidence and constraints; do not forward the full chat by default. Browser login remains with Chrome. Do not read or copy login credentials.

`pro_research` saves a question and returns a request ID immediately. Keep this ID. Use `previous` with a completed request ID to continue its ChatGPT conversation. Different research threads use separate first requests. The same ID must always identify the same input.

Use `pro_status` for state and `pro_result` for the complete answer. Do other useful work while research runs. Avoid frequent status checks. After a disconnect or timeout, call `pro_resume` with the saved ID. Never create a replacement request to retry an uncertain Send. A missing result does not mean the question was not sent.

Return the source conversation and known execution identity with the answer. A picker label alone does not prove which model answered. If Pro is unavailable or the provider requests login or a human verification, report that state. Do not substitute another model or try to bypass verification.
