# Show the account before an MCP approval

The inherited MCP form showed the connected account link from native Apps request
metadata. The current settings panel showed that identity only after a separate
settings read. Restore the immediate label and retain the current settings panel.
Use only `codex_apps` and `_meta.link_id`. Tool arguments must not supply the
account label. Apply the same ID bounds as the existing settings-panel guard.

## Reconciliation

Compare the complete preserved and current `chief_mcp_forms.rs` files. Retain the
current settings read, malformed-schema rejection and unsupported-preview checks.
Restore coverage for all three accepted form mode spellings. Keep defaults out
of answers, retain drafts for the same request and reject stale submissions.
The tool-parameter formatting change is equivalent. Close only this file row.

This is presentation within the optional Apps integration. If that integration
is retained, users must be able to see which supplied account metadata applies
before they approve. The change adds no policy owner and sends no settings write.

## Native reference and limits

Read fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`:
`codex-rs/protocol/src/mcp_approval_meta.rs`,
`codex-rs/codex-mcp/src/auth_elicitation.rs` and
`codex-rs/codex-mcp/src/resource_origin.rs`. Native account metadata and tool
arguments have separate meanings. This patch restores the existing local display
contract; it does not infer identity from tool arguments or add a nested auth
metadata decoder. It does not change the installed binary or resolve the separate
native child-MCP qualification failures.

## Evidence

The rendered regression fails before the repair because no account context is
present before settings are read. Log: `/tmp/decodex-mcp-account-before.log`.
All four form tests pass, including rendered account context before a settings
read. Strict GPUI lint passes with all features and targets. Logs:
`/tmp/decodex-mcp-account-final.log` and `/tmp/decodex-mcp-account-clippy.log`.
An initial post-fix test run lacked the GPUI debug selector; adding that test
selector made the rendered assertion observe the restored label.
Signed desktop acceptance remains in the final catch-up pass.
