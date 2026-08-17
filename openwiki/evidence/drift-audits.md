# Drift Audits

Evidence pages are public-safe proof notes for claims that need durable reuse across OpenWiki pages. They do not replace source, tests, runtime SQLite, private evidence, tracker state, or operator commands as implementation authority. Keep private runtime evidence in Decodex storage; keep OpenWiki evidence limited to source anchors, reverse checks, validation/readback commands, status, and stop conditions.

This page preserves the evidence shape from the historical `docs/evidence/` drift-audit notes and currently tracks the MCP remote-control/productization claim set.

## Current audit: MCP remote control productization

Status: active; current source-inspection verdict is pass.

The OpenWiki and README claims for `decodex mcp serve --transport streamable-http` match current source: Streamable HTTP is loopback-first, capability-profiled, session-tracked, bearer-guarded when exposure risk increases, and covered by unit plus process smoke tests. OAuth Protected Resource Metadata is not a current Decodex claim; the built-in bearer guard is the direct-listener boundary for Decodex-owned Streamable HTTP.

## Watched claims

- Streamable HTTP binds to `127.0.0.1:8193` by default and defaults to the `observe` capability profile; stdio defaults to `admin` (`apps/decodex/src/mcp.rs`, `apps/decodex/src/mcp/types.rs`).
- Streamable HTTP serves `POST /mcp`, validates browser `Origin` against loopback or explicit `--allow-origin`, issues `Mcp-Session-Id` on successful `initialize`, and requires a known session afterward (`apps/decodex/src/mcp/http/security.rs`, `apps/decodex/src/mcp/tests/http/session.rs`).
- `Mcp-Session-Id` is protocol state, not authorization; `--allow-origin` is CORS trust, not authentication (`README.md`).
- Non-loopback listeners require both `--allow-origin` and `--bearer-token-env`; elevated Streamable HTTP profiles require `--bearer-token-env` even on loopback (`apps/decodex/src/mcp/http/security.rs`).
- Bearer auth validates `Authorization: Bearer <token>` for protected requests while allowing unauthenticated CORS preflight (`apps/decodex/src/mcp/http/auth.rs`, `apps/decodex/src/mcp/tests/http/cors_auth.rs`).
- `tools/list` is profile-filtered: `observe` exposes `decodex_observe`, `plan` adds planning/autonomy tools, `operate` adds `decodex_lane_control`, and `admin` adds `decodex_project_control` (`apps/decodex/src/mcp/tools/profiles.rs`).
- `decodex_lane_control` remains inspect-first and guarded; `decodex_project_control scan` refuses to the operator control loop instead of adding an MCP-only scheduling shortcut (`apps/decodex/src/mcp/control/lane/`, `apps/decodex/src/mcp/control/project/server.rs`).
- Public-safe observation must exclude hidden reasoning, private evidence payloads, host paths, secret material, raw steer text, and internal Program graph identifiers (`apps/decodex/src/mcp/observability/`, `README.md`).
- Process-level smoke coverage starts the real binary and checks Streamable HTTP initialize, observe-profile tool discovery, above-profile refusal, SSE progress, and stdout/stderr cleanliness (`apps/decodex/tests/mcp_stdio/process.rs`).

## Reverse checks and drift-audit method

Use reverse checks when changing MCP, README MCP claims, operator-control docs, or remote-control examples:

```sh
git grep -n -E "bearer|Authorization|allow-origin|capability-profile|streamable-http|Mcp-Session-Id|decodex_lane_control|decodex_project_control|manual_attention|retained_resume" -- README.md apps/decodex/src apps/decodex/tests openwiki

git grep -n -E "streamable_http|mcp_streamable_http_process|observe_profile|bearer_auth|elevated_profile|bind_guard|Mcp-Session-Id" -- apps/decodex/src/mcp apps/decodex/tests/mcp_stdio
```

Then read the owning source before editing docs:

- CLI surface: `apps/decodex/src/cli/control_commands/mcp.rs`.
- Transport/profile defaults and protocol constants: `apps/decodex/src/mcp.rs`, `apps/decodex/src/mcp/types.rs`.
- HTTP origin/auth/session/SSE behavior: `apps/decodex/src/mcp/http/`.
- Tool profile and refusal behavior: `apps/decodex/src/mcp/tools/`, `apps/decodex/src/mcp/control/`.
- Product-facing claims: `README.md`, `openwiki/architecture/runtime-architecture.md`, `openwiki/workflows/runtime-operator-workflows.md`.

## Validation and readback commands

Run the narrow checks that match the changed surface:

```sh
cargo test -p decodex mcp::tests::http:: -- --nocapture
cargo test -p decodex --test mcp_stdio mcp_streamable_http_process_observe_profile_smoke -- --nocapture
cargo test -p decodex --test mcp_stdio mcp_stdio_process_stdout_contains_only_json_rpc -- --nocapture
```

For operator-facing readback after a local build, use:

```sh
decodex mcp serve --transport stdio
decodex mcp serve --transport streamable-http --listen-address 127.0.0.1:8193
decodex mcp serve --transport streamable-http --capability-profile operate --bearer-token-env DECODEX_MCP_TOKEN
```

Replace the placeholder environment variable name with a real non-secret environment variable name in local testing, and do not document token values.

## Stop conditions

Stop and update this audit, the README, and the relevant OpenWiki MCP sections in the same lane if any of these change:

- Decodex implements OAuth Protected Resource Metadata or delegates Streamable HTTP auth to a managed relay.
- The stable MCP protocol version, session model, `Mcp-Session-Id` behavior, or SSE framing changes.
- Streamable HTTP no longer requires bearer auth for non-loopback listeners or elevated profiles.
- `--allow-origin`, bearer auth, or profile filtering semantics change.
- MCP gains scheduling, scan, manual-attention, retained-resume, landing, or tracker-write shortcuts that bypass canonical operator/runtime paths.
- Public-safe observability starts exposing private evidence, hidden reasoning, local host paths, secret material, raw steer text, or internal Program graph identifiers.
- Process-level smoke tests stop covering the real Streamable HTTP binary path.
