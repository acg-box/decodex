# Agent terminology and migration

## Current model

Main is the default Agent. L0, L1, L2, and later levels are tree depth, not separate
agent types or fixed permissions. Managed children have one parent. Native Codex
subagents remain native threads; the tree observes their parent relationships
without creating duplicate managed work. The graph shows work dependencies.

The coordination role still controls tool authority: an Agent may organize its
own work, but a sibling cannot manage another sibling's work. Task/goal kinds
remain work semantics. This change does not replace the native subagent system.

## Current names

The desktop surface, runtime coordinator, protocol types and queries, CLI command
(`decodex agent`), files, and SQL objects use Agent names. Protocol 2.96 requires
matching local clients and service. Ship the app, helper, and native libraries
together. New native threads expose only `agent_*` coordination tools.

## Clean installation baseline

Schema 48 is the canonical baseline. Schema 49 adds the current upstream model-source, native-settings, and voice-retirement fields. Earlier migration sources remain in Git
history only. The explicit one-time local conversion preserves accounts and
settings, removes the retired Agent conversations, and replaces the migration
ledger after schema and integrity checks. Older stores are rejected until converted;
startup does not guess how to repair a historical ledger.

All new conversations use current Agent tools. No former-name tool dispatcher or
configuration alias remains. The user requested recreation from original user
inputs; assistant outputs and machine-generated coordination events are not replayed
as user instructions. New responses are actual model output. Unrelated Codex chats
and account data are outside the reset scope.

The coordinated app and database update needs a stopped service. Keep a private
pre-conversion backup for failure recovery. Do not run an older app on schema 48.

## Native reference

Official `openai/codex` main at `0fbf0bedc25d0effec4b758030772468339d315c`
exposes `dynamic_tools` in `ThreadStartParams`, but not `ThreadResumeParams`, in
`codex-rs/app-server-protocol/src/protocol/v2/thread.rs`.
The experimental schema generated from installed `codex-cli 0.158.0-alpha.2`
confirms the same boundary. Source review and installed protocol support are
separate evidence; this rename does not add a new provider capability.
