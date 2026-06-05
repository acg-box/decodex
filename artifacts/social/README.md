# Social Artifacts

This directory stores checked-in Publisher artifacts for external social channels.

- `x/posts/` holds `social_post/v1` publication, block, skip, and failure records for
  X.
- `x/images/` is legacy or explicit operator-approved sample storage only. Publisher
  automation should not commit generated images by default; use X status/media URLs and
  optional content hashes instead.

Social candidates live under `artifacts/github/social-candidates/`; this directory holds
terminal Publisher outcomes. The governing publication contract is
`docs/spec/social-publishing.md`. The primary publishing account is `@decodexspace`;
the controller account is `@hackink`.
