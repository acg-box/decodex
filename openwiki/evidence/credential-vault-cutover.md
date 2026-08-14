# Credential Vault Cutover Evidence

Status: accepted historical donor evidence. redb is no longer the target credential
authority; the current result and transfer boundary are recorded in
[SQLite local-product evidence](sqlite-local-product.md).

Date: 2026-08-14.

Source baseline: `82d942813017ce15bf0a2efef390a76498baa24c`.

Implementation branch: `xv/account-credential-vault-v1`.

This evidence contains no token, email address, account identifier, provider identifier,
credential fingerprint, or secret value.

## Result

The macOS Account Lifecycle credential authority moved from Keychain generic-password
items to one daemon-owned redb database at
`~/.decodex/server/credentials.redb`.

former server store remains the credential-negative Account Registry. It stores account identity,
revision, lifecycle state, routing, quota observations, provider binding, credential
version, and fingerprint evidence. It does not store a token or credential blob.

`decodexd` is the only normal vault reader and writer. GPUI, the native app, the menu bar,
Swift, and the CLI remain protocol clients. One Codex App Server process still has one
immutable Account UUID and provider binding for its lifetime.

## Protected offline transfer

The normal service was stopped. A temporary signed transfer process read only the six
exact registry-bound Keychain records and wrote the complete destination set in one
immediate redb transaction.

The first bounded result was:

```json
{"account_count":6,"classification":"migrated"}
```

An immediate replay produced:

```json
{"account_count":6,"classification":"already_exact"}
```

The replay proves that the destination already contained the exact complete registry
snapshot. A divergent record would have refused the operation. The temporary transfer
feature, module, and command were then removed. The final product has no migration API,
dual read, or Keychain fallback.

The old Keychain items remain unchanged as out-of-band rollback evidence. Their deletion
requires separate user authority.

## Vault and installation boundary

The installed vault had these value-free properties after transfer and restart:

- owner UID `501`;
- mode `0600`;
- one hard link; and
- size `565248` bytes.

Core opens the fixed path through its descriptor-anchored no-follow helper. It refuses
wrong ownership, unsafe permissions, a non-regular file, a link count other than one, and
symbolic-link traversal. Runtime gives the already-open file to `redb`. Every accepted
write uses immediate durability.

The final LaunchAgent starts `/Users/x/.local/bin/decodexd` directly. The installed
binary passed strict signature verification, uses hardened runtime, has identifier
`box.acg.decodex.daemon`, and has no entitlement payload. Normal startup has no daemon
app bundle, embedded development provisioning profile, Keychain access group, or Python
wrapper. Both LaunchAgent working-directory fields use the primary repository, not the
task worktree.

## Restart and real Codex proof

After service restart, the credential-vault doctor check was ready. All six enabled
accounts were lifecycle-ready and had exact registry bindings. Product Store and Quick
Task composition were also ready. The overall doctor remained unavailable only because
ManagedRepository was explicitly disabled.

A temporary test-only adapter reused the production `ManualAccountLauncher` and
`ReadOnlyProbe`. It selected the existing fixed account, read its exact redb record,
started the canonical signed Codex App Server, projected ChatGPT authentication through
process-scoped `account/login/start`, and completed `initialize`, `account/read`, and
`thread/list`. The value-free result was:

```json
{"account_count":1,"account_read":true,"classification":"authenticated","initialize":true,"process_spawned":true,"thread_list":true}
```

The probe shut down its child and the temporary adapter was removed. This proves the
complete vault-to-real-Codex path without adding a permanent operator command.

## Independent Quick Task finding

A real fixed-account Quick Task command was accepted and persisted before the direct
probe. It remained `routing_pending` after two explicit resume attempts and spawned no
Codex child. Every account had a current seven-day quota observation, but the provider
did not supply a five-hour window. The current selector requires both windows to be
current and therefore failed closed.

This is an existing quota-contract issue, not credential-vault evidence. The cutover did
not change fixed or balanced routing, quota policy, fallback, or scheduler behavior. A
later milestone must define how a missing provider quota window maps into the routing
ontology before full Quick Task acceptance can pass.

## Focused verification

The following checks passed during cutover:

- 22 macOS local-service installer tests, including direct executable authority refusal;
- two credential-vault typed-path and unsafe-file tests;
- three redb store, restart, and one-writer tests;
- all `decodex-core` and `decodex-runtime` unit and non-ignored integration tests;
- `cargo clippy -p decodex-core -p decodex-runtime -p decodexd --all-targets
  --all-features -- -D warnings`;
- `cargo check --all-features --all-targets --workspace` with Xcode beta selected so
  GPUI could compile its Metal shaders;
- the 24-test `cargo make test-vnext-architecture` gate;
- a locked resolved graph with exactly `redb` 4.1.0 and default features disabled;
- `cargo audit --json`, with zero vulnerabilities and no new redb advisory;
- strict installed-daemon code-signing verification; and
- the one-account real Codex App Server probe above.

Stable `cargo fmt --all -- --check` is not a clean repository gate at this baseline. The
checked-in rustfmt configuration requires nightly-only options, and stable rustfmt
proposes unrelated rewrites across the workspace. The project requires the stable Rust
channel, so this cutover did not run a nightly bulk rewrite. `git diff --check`, Clippy,
compilation, and the scoped tests above are clean.

## Security limit

The redb file is plaintext at the application layer. This version relies on owner-only
filesystem access and host disk encryption. It does not protect against root or a
malicious process that already runs as the same user. Application-layer encryption and
key rotation are separate future work.
