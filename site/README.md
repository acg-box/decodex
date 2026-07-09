# Decodex Site

This directory owns the static public Decodex product site and app download entry.
It is an Astro/TypeScript surface and must stay independent from live Decodex daemon
state.

Current scope:

- Astro + TypeScript site rendering
- Tailwind-powered global styling
- public product homepage and app download content
- static assets and content owned by the site build

Local commands:

- `npm install`
- `npm run dev`
- `npm run build`
- `npm run check`

External automation owns publication to GitHub Pages. Runtime scheduling, tracker
writes, local operator state, app-server orchestration, and Radar/Publisher
automation remain outside this static site boundary.
