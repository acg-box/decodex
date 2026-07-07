---
type: "Runbook"
title: "GitHub Pages Deploy"
description: "Procedure for publishing the Decodex static site to GitHub Pages."
status: active
authority: procedural
owner: automation
tags: [runbook]
last_verified: 2026-06-17
---
# GitHub Pages Deploy

Goal: Define how Decodex is published as a static GitHub Pages project site for
`decodex.space` without repository-owned GitHub Actions.

Read this when:
- You need to enable or verify GitHub Pages deployment for Decodex.
- You are configuring the `decodex.space` custom domain.
- You need to know which settings live in GitHub/DNS versus which source files are
  committed in this repository.

Inputs:
- GitHub admin access for the `hack-ink/decodex` repository
- DNS control for `decodex.space`
- The Astro site source under `site/`
- The external Decodex automation operations directory under
  `/Users/x/Documents/automations/decodex`

Depends on:
- `site/astro.config.mjs`
- `site/package.json`
- External Codex automation, not `.github/workflows/`

Outputs:
- A static Pages publication driven by external Codex automation
- A GitHub Pages custom-domain configuration for `decodex.space`
- DNS records that point the domain at GitHub Pages

## Pages model

Decodex is a **repository-scoped project Pages site**, not the organization root
Pages site.

- Repository: `hack-ink/decodex`
- Default GitHub Pages URL before the custom domain: `https://hack-ink.github.io/decodex`
- Production custom domain: `https://decodex.space`

The custom domain is configured in this repository's `Settings -> Pages`. That
means the site remains repo-scoped even though GitHub Pages still uses the
standard GitHub Pages DNS infrastructure underneath.

## Repository-side contract

This repository owns the source and local validation for the static site. It does not
own GitHub Actions workflow files, Dependabot automation, upstream monitoring, or
public publishing automation.

Before publication, validate the site from the repository root:

```bash
cargo make check-node
cargo make build-node
```

The external Codex automation under `/Users/x/Documents/automations/decodex` owns
the publication procedure. It may rebuild the site from a clean checkout or consume a
fresh `site/dist` generated from the current source, then publish through the
configured GitHub Pages mechanism. Do not reintroduce `.github/workflows/` for this
repository.

## GitHub repository settings

The repo still needs manual GitHub configuration:

1. Open `Settings -> Pages`
2. Configure the publishing source expected by the external Codex automation, such
   as a Pages branch if the automation publishes one
3. Under `Custom domain`, set `decodex.space`
4. After DNS is live, enable `Enforce HTTPS`

Important:

- Do not add a repo-main `CNAME` file unless the chosen Pages source explicitly reads
  from the main branch.
- If the automation publishes a Pages branch or artifact, that automation owns the
  deployed `CNAME` placement required by that publishing mode.

## DNS for `decodex.space`

`decodex.space` is an apex domain. If you only care about the apex host and do
not need `www`, use the apex records only and skip any extra aliasing.

Recommended apex setup:
- `A` for `@` -> `185.199.108.153`
- `A` for `@` -> `185.199.109.153`
- `A` for `@` -> `185.199.110.153`
- `A` for `@` -> `185.199.111.153`
- Optional `AAAA` for `@` -> `2606:50c0:8000::153`
- Optional `AAAA` for `@` -> `2606:50c0:8001::153`
- Optional `AAAA` for `@` -> `2606:50c0:8002::153`
- Optional `AAAA` for `@` -> `2606:50c0:8003::153`

Optional `www` support:
- If you also want `www.decodex.space`, GitHub Pages still expects the `www`
  `CNAME` to point at the standard owner/organization Pages host:
  - `CNAME` for `www` -> `hack-ink.github.io`
- This does **not** turn Decodex into the organization root site. It is only the
  DNS target GitHub uses for Pages routing. The custom domain remains attached
  to the `hack-ink/decodex` repository in `Settings -> Pages`.

If you do not want to expose or maintain a `www` variant, omit it entirely and
serve only `https://decodex.space`.

## Verification

After external automation publishes and DNS propagates:

1. Confirm the external Codex automation run completed and recorded its output under
   `/Users/x/Documents/automations/decodex`
2. Verify the custom domain is accepted in `Settings -> Pages`
3. Check that this repository still has no GitHub Actions workflows:

```bash
test ! -d .github/workflows
```

4. Check DNS:

```bash
dig decodex.space +noall +answer -t A
dig decodex.space +noall +answer -t AAAA
dig www.decodex.space +noall +answer -t CNAME
```

5. Verify the production URL:

```bash
curl -I https://decodex.space
```

## Remaining manual ownership

These steps cannot be finished purely by editing repo files:

- Selecting the Pages publishing source expected by the external Codex automation
- Setting `decodex.space` in this repository's `Settings -> Pages`
- Verifying the domain in GitHub if needed
- Creating or updating the DNS records at the domain provider
