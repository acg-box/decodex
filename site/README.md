# Decodex Site

This directory owns the static public Decodex product site.
The related-software widget links to OpenAI Codex beta builds from OpenAI's appcast.
Decodex build instructions remain in the repository.
It is an Astro/TypeScript surface and must stay independent from live Decodex daemon
state.

Current scope:

- Astro + TypeScript site rendering
- Tailwind-powered global styling
- public product homepage and related OpenAI Codex download content
- static assets and content owned by the site build

Local commands:

- `npm ci --ignore-scripts`
- `npm run dev`
- `npm run build`
- `npm run check`

Publication is separate from the local site build. Runtime scheduling, tracker
writes, local operator state, app-server orchestration, and Radar/Publisher
automation remain outside this static site boundary.
