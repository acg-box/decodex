# Current login-policy classification

Native capability: available and qualified through the installed process.
Decodex independent browser/device-code enrollment: not integrated with this
native policy; do not advertise enforcement. AccountLoginManager still uses the
separate account-login engine. Applicability and the enrollment policy authority
remain unresolved in the adoption register and must stay visible for the user's
scope decision. Reading an arbitrary task's policy is not a valid substitute.

The restored native tests cover running policy and cold restart; they do not
prove desktop enrollment enforcement. See [current reconciliation](native-policy-routing-reconciliation.md).

## Preserved historical record

The original note follows unchanged. Its installed version and implementation
status are historical, not current completion evidence.

# Effective login methods

Upstream commit `a20092a7a22cead38b46f31de07ee87705cb9b4e` adds
`configRequirements/read.requirements.allowedLoginMethods`. The field reports the
running authentication manager's policy after managed, forced-login and workspace
restrictions. An empty array permits no method. Missing or null fields on older
servers do not prove permission. The unrestricted default can still return
`requirements: null`. A file edit does not change the current process's policy.

The installed alpha.16 schema includes this field. The Chief process bridge now
admits the read-only request with empty parameters. The native regression checks
unrestricted, API-only and ChatGPT-only results, edits the configuration while the
process runs, and starts a new process to check the changed policy.

## Remaining integration

Decodex does not yet project this field to its login UI or apply it before account
enrollment. `AccountLoginManager` currently runs the independent
`decodex-account-login` engine for browser and device-code ChatGPT login. It does
not call native `account/login/start`. The new read permission alone does not
close this gap.

Bind the observation to the native process and configuration authority used for
enrollment, including its generation. Do not use an arbitrary active task's
account-bound process as global login policy. Project supported methods to the UI
and recheck the same authority before opening authorization or installing a
credential. Both existing login choices are ChatGPT methods. Keep absent policy,
unavailable policy and an explicit empty allowlist distinct. Native enforcement
remains authoritative for native login calls; do not expose those mutations
through the conversation bridge as a test shortcut.

Acceptance must cover restricted and empty policies, stale process observations,
restart, browser/device-code entry points, concurrent clients and the actual
desktop controls. The current native read test does not qualify those flows or
managed MDM/cloud policy delivery.
