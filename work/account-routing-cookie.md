# Retain account API routing affinity

Classification: core compatibility for the existing account HTTP consumer.
Fixed upstream: 595cc91e8cbb1c2ca822d0311dcf12709410c582.
Reference: codex-rs/http-client/src/chatgpt_cloudflare_cookies.rs and its tests.
The upstream implementation identifies __oailb as an infrastructure routing cookie,
separate from account and session cookies.

The shared AccountApiRuntime HTTP client previously discarded Set-Cookie headers.
It now retains only __oailb from HTTPS responses at the exact chatgpt.com host and
sends it on subsequent matching requests. reqwest's Jar owns parsing, domain,
path and expiry rules. The outbound Cookie header is marked sensitive.

The store lives with the account HTTP client and its clones. It has no disk
persistence. AccountService continues to own bearer credentials and each request's
ChatGPT-Account-Id. Session/auth cookies are excluded before they enter the jar.
The existing timeouts, no-redirect and no-retry policies remain in the same builder.
Native app-server HTTP/WebSocket model traffic keeps its upstream transport owner.
This change adds no configured-cookie API, new host, account setting or UI.

The runtime enables the existing reqwest dependency's cookies feature. Other
workspace consumers do not get a cookie provider by default. The implementation
helper and runtime manifest match the preserved snapshot. The complete account
API source comparison leaves only the extracted client builder and the existing
optional attested activation profile as adaptations. The preserved snapshot
constructor lacked that profile; its old constructor is not restored.

## Dependency evidence

Baseline: adfd2cd6e7f9346b7081df0cf7462966e00e50a3. Cargo generated the lock.
No existing resolved package version changed. Four transitives were added:

| Package | Version | Producer source | Published UTC |
| --- | --- | --- | --- |
| cookie | 0.18.2 | SergioBenitez/cookie-rs | 2026-08-08 |
| cookie_store | 0.22.1 | pfernie/cookie_store | 2026-02-16 |
| psl-types | 2.0.11 | addr-rs/psl-types | 2022-08-10 |
| publicsuffix | 2.3.0 | rushmorem/publicsuffix | 2024-11-14 |

Checks observed on 2026-09-25 at 21:22 UTC:

| Check | Status | Evidence |
| --- | --- | --- |
| Baseline/final vulnerabilities and known malicious-package advisories | checked-no-record | Cargo audit against RustSec e2111519ba6d14a5da59a7b2e5c8083ae8a37c01; zero vulnerabilities in either graph |
| Historical advisory residuals | finding | Four unmaintained packages and yanked chacha20 0.10.1 unchanged; see dependency-security-repair.md |
| Artifact and registry identity | checked-no-record | Each crate archive SHA256 matches the generated lock and live crates.io metadata; all four versions are non-yanked |
| Producer source identity | checked-no-record | Each embedded VCS commit resolves in the producer repository listed above |
| Build and runtime behavior | checked-no-record | Reviewed source and manifest owners before build; cookie's build script checks compiler doc_cfg support; the other three have no build script |
| Artifact signatures | unsupported | Cargo registry checksum verification is used; signed provenance is not claimed |

The selected crates implement parsing and in-memory storage. No new native build,
binary download or network service is required. cookie_store's secure-cookie value
logging feature is not enabled. The wrapper rejects authentication cookies before
calling the store. Source identity and integrity checks are not proof of benign
behavior or an approval of historical residual findings.

risk_coverage: complete for the scoped checks and four resolved identities.
risk_delta: unchanged; no new or worsened advisory finding observed.
decision: proceed with the scoped cookie feature.

## Validation boundary

Tests cover cross-path routing, expiration, rejected session cookies, sensitive
headers, and rejection of foreign hosts and insecure sources or targets. The
client wiring is in account_http_client; reqwest's existing CookieService reads
and writes the supplied store. No production account request is made for this
validation. Native transport and signed desktop acceptance are separate work.

Validation results: two focused cookie tests passed. Stable Rust workspace nextest
passed 2,456 tests with 64 skipped; strict Clippy passed all 12 workspace packages,
all targets and features. Nextest marked two existing cache/publisher tests leaky
in the full run. A serial recheck passed all three matching test instances without
a leak marker, and no task test process remained. No source change was made for
that non-reproduced observation.

Automation remains paused, including after manual completion.
