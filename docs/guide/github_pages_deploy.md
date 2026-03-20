# GitHub Pages Deploy

Goal: Define how Decodex is published as this repository's GitHub Pages project
site from Actions and what still must be configured manually for the
`decodex.space` custom domain.

Read this when:
- You need to enable or verify GitHub Pages deployment for Decodex.
- You are configuring the `decodex.space` custom domain.
- You need to know which settings live in GitHub/DNS versus which ones are committed in the repo.

Inputs:
- GitHub admin access for the `hack-ink/decodex` repository
- DNS control for `decodex.space`
- The deployed Astro site under `site/`

Depends on:
- `.github/workflows/deploy-pages.yml`
- `site/astro.config.mjs`

Outputs:
- A Pages deployment driven by Actions on pushes to `main`
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

## Repository-side deployment

The repository publishes the static site with:

- `.github/workflows/deploy-pages.yml`

This workflow:

1. Checks out `main`
2. Installs Node dependencies for `site/`
3. Builds the Astro site into `site/dist`
4. Uploads `site/dist` as the Pages artifact
5. Deploys the artifact with `actions/deploy-pages`

## GitHub repository settings

After the workflow exists, the repo still needs manual GitHub configuration:

1. Open `Settings -> Pages`
2. Set the source to `GitHub Actions`
3. Under `Custom domain`, set `decodex.space`
4. After DNS is live, enable `Enforce HTTPS`

Important:

- When publishing from a custom GitHub Actions workflow, GitHub does **not** create
  or require a checked-in `CNAME` file.
- Any existing `CNAME` file is ignored for this publishing mode.

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

After the workflow runs and DNS propagates:

1. Confirm the Pages workflow succeeds in Actions
2. Open the Pages environment URL in the deploy job output
3. Verify the custom domain is accepted in `Settings -> Pages`
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

- Switching this repository's Pages source to `GitHub Actions`
- Setting `decodex.space` in this repository's `Settings -> Pages`
- Verifying the domain in GitHub if needed
- Creating/updating the DNS records at the domain provider
