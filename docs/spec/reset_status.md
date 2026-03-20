# Reset Status

Purpose: Define the published Decodex reset-status schema for a third-party,
community-observed Codex reset signal rendered in the homepage status slot.

Status: normative

Read this when:
- You are fetching or validating the upstream community reset tracker.
- You are rendering the homepage reset-status widget.
- You need to know how Decodex should frame a non-official reset signal.

Not this document:
- The GitHub change-bundle schema.
- The signal-entry schema.
- The release-delta schema.

Defines:
- The canonical `reset_status/v1` shape.
- How upstream third-party payloads are normalized for the site.
- The required source-framing rules so the widget does not imply official
  OpenAI confirmation.

## Entry identity

The canonical schema identifier is:

- `reset_status/v1`

## Required fields

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `reset_status/v1`. |
| `source_label` | string | Human-readable source label, such as `Community tracker`. |
| `source_kind` | string | Must be `community`. |
| `source_url` | string | Public site URL for the tracker. |
| `source_api_url` | string | API endpoint used to fetch the status. |
| `status` | string | Normalized status: `reset`, `not_reset`, or `unknown`. |
| `stale` | boolean | Whether the upstream observation is too old to trust as current status. |
| `configured` | boolean | Whether the upstream tracker reports itself configured. |
| `upstream_state` | string | Raw upstream state value when available, such as `yes` or `no`. |
| `updated_at` | string | UTC timestamp representing the tracker-provided freshness marker. |

## Optional fields

The artifact may also contain:

- `auto_reset_hours`
- `reset_at`

If `reset_at` is present, it must be a UTC timestamp string.

## Normalization rules

Decodex must normalize the upstream tracker response into these meanings:

- upstream `state = "yes"` -> `status = "reset"`
- upstream `state = "no"` -> `status = "not_reset"`
- any other or missing upstream state -> `status = "unknown"`

`updated_at` must be derived from the upstream freshness marker when available.
If the upstream tracker reports timestamps as Unix milliseconds, the published
artifact must convert them into UTC ISO 8601 strings.

## Freshness rule

The artifact must mark the source as stale when the upstream observation age is
older than a reasonable current-status window. The default rule is:

- if `auto_reset_hours` is present, stale when the observation age exceeds
  `auto_reset_hours * 2`
- otherwise stale when the observation age exceeds `48` hours

Homepage rendering should treat stale data as unclear current status even if the
raw normalized `status` is still `reset` or `not_reset`.

## Source framing rule

The published artifact and any homepage rendering must frame this signal as:

- third-party
- community-observed
- not an official OpenAI reset confirmation

The widget may summarize the observed state, but it must not use wording that
implies OpenAI directly confirmed a reset through an official source.

## Homepage rendering rule

When a valid `reset_status/v1` artifact exists, the homepage should render a
small global reset-status slot phrased as a direct user question:

- `Are we reset today?`
- `Yes` when `status = "reset"`
- `No` when `status = "not_reset"` or `status = "unknown"`

The published artifact must still retain freshness and source fields for
traceability, but the default homepage UI does not need to surface that extra
metadata inline.
