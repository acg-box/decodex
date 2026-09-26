# Filesystem policy cwd

## Current qualification on 2026-09-26

Classification: native-owned execution behavior. Decodex does not implement an
executor filesystem or a second permission evaluator. Restore the inherited
standalone probe without code changes. It is opt-in and is not a product control.

Run it with an explicit installed binary:

```sh
python3 scripts/vnext/codex_exec_policy_probe.py --binary /Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex
```

All ten checks pass on installed `codex-cli 0.158.0-alpha.2`; executor metadata now
also reports `0.158.0-alpha.2`. Dynamic permissions without cwd return `-32602`.
The probe uses a private temporary HOME/CODEX_HOME, synthetic files and stdio. It
makes no model request and uses no account credentials. It waits for its executor
to stop before the temporary directory is removed.

The checks cover modern and legacy allowed reads, explicit read denial, missing
cwd rejection, and full-disk reads with denied write, mkdir, remove and copy.
The denied source content stays unchanged and no destination appears. They do
not prove every internal dispatch branch, remote Windows behavior, Chief tool
execution, apply-patch behavior or signed desktop acceptance.

Fresh fixed-cutoff source inspection confirms these owners at
`595cc91e8cbb1c2ca822d0311dcf12709410c582`:

- `exec-server/src/server/registry.rs::resolve_filesystem_sandbox` rejects dynamic
  policy without cwd and resolves the legacy static-policy fallback.
- `file-system/src/lib.rs::WireFileSystemSandboxContext` retains the explicit
  policy context and requires cwd for relative patterns and project roots.
- `exec-server/src/local_file_system.rs` validates paths before selecting separate
  read and write implementations. The upstream read-only write-denial test checks
  both local and remote implementations; that upstream test was read, not run.
- `file-system/src/environment_accessor.rs` binds every operation to its captured
  sandbox. Cache equality includes filesystem identity and the complete context.

Evidence: `/tmp/decodex-native-filesystem-158.log`. This current result supersedes
only the installed-version and probe-availability statements below. Preserve the
inherited review and its acceptance limits as historical evidence. It does not
close the other native execution/security items in R10. Automations stay paused.

## Preserved earlier review

Upstream `841b5490b2517a8d86a1ead7bd9790f11176e363` makes selected policy cwd
mandatory internally and separates it from a helper process's launch directory.
Filesystem helpers can start at the filesystem root while permission rules remain
bound to a removed selected directory. Windows keeps drive/share roots and binds
relative deny globs before moving the helper; home-relative patterns remain such.

Filesystem wire requests carry additive `policyContext`; old dynamic policies
require cwd, while old static policies can use the executor's own cwd. Process
requests can use their mandatory process cwd when legacy sandbox cwd is absent.
Foreign path URIs remain transportable and are validated on the selected executor,
including before unsandboxed filesystem selection. At the cutoff, later read/write
specific sandbox dispatch retains that validation. Local Decodex has no executor
wire/context implementation to replace; native app-server owns this behavior.

Run the isolated installed-executor check with:

```sh
python3 scripts/vnext/codex_exec_policy_probe.py --binary /Applications/ChatGPT.app/Contents/Resources/codex
```

It uses a temporary home, synthetic files and stdio, without account credentials
or model inference. It removes only its empty selected directory. Modern and
legacy allowed reads succeed, explicitly denied reads fail, and a dynamic policy
without cwd fails. The denied fixture remains unchanged. Five checks pass in
`/tmp/decodex-1441-executor-policy-versioned.log`. Python compilation and diff
whitespace checks pass.

The installed CLI is `0.155.0-alpha.16.3`; executor metadata reports `0.0.0`.
The installed missing-cwd error is `-32600`, whereas the fixed source expects
`-32602`. The probe requires the rejection message as well as either known code.
Initial probe failures used the wrong special-path enum shape and assumed the
source error code; these fixture assumptions were corrected without changing
production sandbox behavior.

This does not prove the complete Chief/apply-patch path, remote Windows behavior,
relative-denial behavior on every platform, or signed desktop interaction. The
upstream corresponding tests were read, not run locally. Those acceptance scopes
remain distinct from this native executor result.

## Independent read and write permissions

Upstream `a4ee536f0132092f20996c3810c14d090e4dded1` selects a sandbox separately
for read and write operations. Full-disk reads can use direct access; restricted
mutations still use the platform sandbox. Executor path conventions control
`:slash_tmp` rules. Foreign paths are still validated before dispatch. Remote
filesystem calls preserve the complete sandbox context. Capability discovery and
skill reads use the read-specific check. Final source retains these decisions;
later changes add environment accessors and remove private-desktop fields.

The installed-executor probe now has ten checks. With full-disk read permission,
reading succeeds while write, mkdir, remove and copy fail with permission errors.
The source file is unchanged and no destination exists after each denied call.
All ten checks pass on `0.155.0-alpha.16.3` in
`/tmp/decodex-1444-executor-read-write.log`. This proves permission behavior over
native stdio, not which internal dispatch branch ran or operation without sandbox
runtime support. The upstream direct-read/no-helper and old remote discovery
tests were inspected but not run here. Windows execution remains unverified.

## Bound filesystem accessors

Upstream `3d3ae4965ab370217e871b3a7f0d15589557ee4b` adds a borrowed
`EnvironmentAccess` API. Each operation forwards captured permissions; consumers
cannot extract the filesystem or supply a different sandbox. Cache keys compare
filesystem identity and the complete context without keeping the filesystem
alive. Already-open read streams may outlive the accessor and do not reauthorize
each read. The explicit unrestricted constructor remains available for internal
operations and callers pending migration; this commit alone does not prove every
discovery caller uses captured permissions.

Read all 280 implementation lines and local/remote text, stream and cache-key
tests. The accessor is unchanged at the cutoff. Decodex links neither this crate
nor exec-server internals; app-server owns these filesystem operations. There is
no new public protocol method or local accessor replacement to implement. Further
upstream consumer migrations must still be reviewed. Native accessor tests were
not executed locally; the prior stdio permission probe has a different scope.
