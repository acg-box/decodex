# Separate source preservation from native qualification

The remaining four native fixture rows are fully compared with the preserved
snapshot. Close their source-comparison disposition without changing any failed
qualification to success. No native binary, fixture assertion or product policy
is changed by this classification.

| File | Complete source difference | Native qualification |
| --- | --- | --- |
| chief_process_native_flex_tests.rs | Both inherited tests and all assertions remain. The shared serve_with_text helper becomes serve_fixture_usage with equivalent synthetic text output and explicit message identity. | Configured Flex passes; explicit thread/settings/update returns Flex live but null on cold resume. Failure remains open. |
| chief/tests/native_subagent_live.rs | The original approval, root-tier and ownership assertions remain. Two additional reads check descendant identity and native child input capability. | Child approval/tier qualification passed in the existing record. This does not qualify human MCP input. |
| chief/tests/native_subagent_mcp.rs | Both inherited human-input markers and strict -32603/root-handoff assertions remain. The fixture now checks both markers before reporting failure and prints their synthetic replies. | Both markers return accept with empty content on the installed binary. This is not the expected root handoff. |
| chief/tests/native_subagent_mcp_server.py | Byte-identical to the inherited file. | A preserved synthetic server does not make the failing native result pass. |

Fresh SHA-256 readback of the installed Codex executable is
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`,
the same binary recorded in the alpha.2.1 qualification reports. No binary change
or new hypothesis warrants repeating the same failing runs in this document batch.
See [Flex evidence](native-flex-qualification.md) and
[child MCP evidence](native-child-mcp-qualification.md) for the actual tests and
their limitations. No ignored ordinary-suite result is counted as qualification.

## Restored top-level record

Read the full inherited/current CODEX_UPSTREAM_REVIEW.md diff. The current file had
lost the later numbered source reviews and several earlier capability notes.
Restore the entire original file without changing its bytes, under a current
notice. The historical cursor and partial implementation instructions do not
override the current fixed-range inventory, feature owners or optional scope.

The restored record retains platform applicability, native ownership and explicit
acceptance gaps for execution, Guardian, model recovery, voice, settings and
history. Later feature notes qualify implemented optional controls separately.
The source review does not prove installation, release, current native policy
execution or the final signed desktop artifact.

## Register semantics

The 360-entry inherited-file register can now record every full source comparison.
Four rows explicitly identify native qualification limits instead of the ambiguous
requires-content-review value. Zero unclassified source rows does not mean zero
remaining delivery work: pending PR merges, R03/R06-R12 review, native limits,
the complete optional removal inventory and fresh signed desktop acceptance
remain separate requirements. Automations remain paused after completion.
