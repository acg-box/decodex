---
name: pro-research
description: Consult ChatGPT Pro for a difficult research or design question when the user requests Pro or this Agent has permission to delegate research to its connected ChatGPT subscription.
---

# Pro research

Use the Decodex Research tools for questions that benefit from an independent, deeper analysis. Keep ordinary execution in this Agent. Send only the task, relevant evidence and constraints; do not forward the full chat by default. Browser login remains with Chrome. Do not read or copy login credentials.

`pro_research` saves a question and returns a request ID immediately. Keep this ID. Use `previous` with a completed request ID to continue its ChatGPT conversation. Different research threads use separate first requests. The same ID must always identify the same input.

Keep the question concise. Write selected source material to a UTF-8 file and pass its absolute path as `contextFile`. Use `prompt` for the question, or `promptFile` when the question itself is in a file. Both files are read once and saved as immutable input. Do not copy their contents into status checks or recovery calls.

Default `delivery: "auto"` sends input up to 128 KiB inline and larger input as one UTF-8 attachment. This is a byte-based transport preference, not a token or model limit. If the question itself is large, the attachment contains the complete question. Explicit `attachment` and `inline` modes are also available. The receipt reports the selected mode and the file's name, size, hash and submitted file ID. File availability does not guarantee that the full source remains in the model's active context.

Continue with `previous` and only the new question. Refer to files already attached in that conversation; do not attach the same source again. Use a new `contextFile` only for additional evidence. Report an unavailable file instead of guessing its contents. A local source-file edit does not change an existing request.

Keep numbered parts as a backup when an attachment cannot be prepared and the saved receipt confirms that no Send was attempted. Send parts through `promptFile` with `delivery: "inline"`, request only an acknowledgment, and use each completed ID as `previous`. Send the research question last. Split at line or section boundaries and keep the source and part paths for coverage checks. Do not resume the unused attachment request after switching to parts. Never change delivery or resend material after an uncertain `Sending` or `Submitted` result.

Pass the active Decodex Project's stable ID and name as `project`. Do not derive identity from a display name or working directory. The component creates and reuses that project's ChatGPT Project. Without a project, research goes to General. Continuations inherit the saved project and cannot move between projects. Project organization does not replace explicit context selection or guarantee project-only memory.

Use `pro_result` for the complete answer. If the receipt has `completionNotification: "decodex"` and the host instructions provide completion events, keep the ID and do independent work; end the current turn when only research remains. Read the result when the host delivers its completion event. Do not poll or sleep in a loop. Other clients can use `pro_status` when they need to check progress. After a disconnect or timeout, call `pro_resume` with the saved ID. Never create a replacement request to retry an uncertain Send. A missing result does not mean the question was not sent.

Return the source conversation and known execution identity with the answer. A picker label alone does not prove which model answered. If Pro is unavailable or the provider requests login or a human verification, report that state. Do not substitute another model or try to bypass verification.
