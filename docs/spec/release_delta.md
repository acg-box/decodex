# Release Delta

Purpose: Define the published Decodex release-delta schema that compares the latest stable release to the latest prerelease for a tracked GitHub lane.

Status: normative

Read this when:
- You are generating or validating release-versus-prerelease summary data.
- You are rendering the homepage release-delta module.
- You need to know how prerelease highlights are mapped back to existing signal entries.

Not this document:
- The GitHub change-bundle schema.
- The signal-entry schema.
- The procedural workflow for collecting GitHub or release data.

Defines:
- The canonical `release_delta/v1` shape.
- Required stable and prerelease release metadata.
- The compare summary between the selected tags.
- The mapping from compare commits to existing signal entries.

## Entry identity

The canonical schema identifier is:

- `release_delta/v1`

## Required fields

| Field | Type | Notes |
| --- | --- | --- |
| `schema` | string | Must be `release_delta/v1`. |
| `repo` | string | Repository in `owner/name` form. |
| `tag_prefix` | string | Prefix used to scope the relevant release channel, such as `rust-v`. |
| `generated_at` | string | UTC timestamp for artifact generation. |
| `stable_release` | object | Latest non-prerelease release for the scoped channel. |
| `prerelease` | object | Latest prerelease release for the scoped channel. |
| `compare` | object | GitHub compare summary from stable tag to prerelease tag. |
| `tracked_signal_slugs` | array | Slugs of already-published signal entries whose source commits appear in the compare set. |

## Release objects

`stable_release` and `prerelease` must both contain:

- `tag_name`
- `name`
- `published_at`
- `url`

The selected releases must satisfy:

- `stable_release` is not a prerelease
- `prerelease` is a prerelease
- both tag names start with `tag_prefix`

## Compare object

`compare` must contain:

- `status`
- `ahead_by`
- `total_commits`
- `url`

`compare` may contain:

- `commit_shas`
- `pr_numbers`

If `commit_shas` is present, it must contain the compare commit SHAs that define the prerelease delta from the chosen stable tag.

If `pr_numbers` is present, it must contain the GitHub pull-request numbers referenced by the compare commits for the chosen delta.

## Signal reuse rule

`tracked_signal_slugs` must only include published `signal_entry/v1` items whose:

- `lane = "github"`
- `source_refs.repo` matches `repo`
- at least one source commit SHA is present in the compare commit set, or
- the signal's primary PR number appears in the compare PR-number set

Signal reuse must be evidence-backed. A release-delta artifact must not claim a signal belongs to the prerelease delta without matching compare evidence.

## Homepage rendering rule

When a valid `release_delta/v1` artifact exists for the homepage lane, the homepage must render:

- the latest stable version
- the latest prerelease version
- the compare magnitude, such as commit count
- the tracked signals that explain what the prerelease unlocks beyond stable

The release-delta module must remain subordinate to the overall page hierarchy. It may summarize the delta, but it must not replace the main signal feed.
