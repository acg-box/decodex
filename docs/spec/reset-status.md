# Reset Status

Purpose: Define the runtime behavior for the homepage reset-status widget.

Status: normative

Read this when:
- You are changing the homepage reset-status widget.
- You are changing how the client fetches OpenAI status data.
- You need to know the current Are-we-reset heuristic.

Defines:
- The upstream source used by the homepage widget.
- The one-fetch-per-page-load rule.
- The current 24-hour alert heuristic for `Yes` versus `No`.

## Source

The widget uses the OpenAI public status page:

- source URL: `https://status.openai.com/incidents/01KK9JA8JKQKDW1W24T09NHBYH`
- API URL: `https://status.openai.com/api/v2/incidents.json`
- incident id: `01KK9JA8JKQKDW1W24T09NHBYH`

The homepage must fetch this API directly from the browser. Do not route this
through a repo-owned static artifact, scheduled workflow, or generated content
collection.

## Runtime fetch rule

The reset-status widget must fetch status data:

- once per page open
- from the client
- without background polling

If multiple reset-status widgets are rendered on one page, they should share
the same in-page request instead of issuing duplicate fetches.

## Heuristic

The homepage question remains:

- `Are we reset today?`

The current decision rule is:

- `Yes` when the `Codex unresponsive` incident shows at least one alert within
  the last `24` hours
- `No` otherwise

For this purpose, the client must first select the incident with id
`01KK9JA8JKQKDW1W24T09NHBYH`. An alert is any incident-level or update-level
timestamp on that incident in the last 24 hours. The client should inspect:

- `incident.created_at`
- `incident.updated_at`
- `incident.monitoring_at`
- `incident.resolved_at`
- `incident_updates[].display_at`
- `incident_updates[].created_at`
- `incident_updates[].updated_at`

## Failure behavior

If the status fetch fails or the payload is malformed:

- keep the widget link pointing at the OpenAI incident page
- render the answer as `No`
- render the widget in the muted state

## Source framing rule

The widget remains a heuristic. It must not claim that the OpenAI status page
directly confirms a Codex quota or usage reset event.
