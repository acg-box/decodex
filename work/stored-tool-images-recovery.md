# Stored tool image references

The Chief timeline marked a native tool-output image as unknown when it used
`file_id` instead of `image_url`. Restore the existing stored-image descriptor.
Keep its original content index and omit the private file ID and image bytes.
Empty or non-string IDs remain unknown. Existing URL handling is unchanged.

## Native contract

At fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`codex-rs/protocol/src/models.rs` defines `FunctionCallOutputContentItem::InputImage`
with a flattened `ImageReference`. That reference accepts either `image_url` or
`file_id`. The schema generated from installed Codex `0.158.0-alpha.2` also has
both forms in `ServerNotification.json`. This change adapts an existing consumer;
it does not add an image-fetch or upload operation.

## Evidence

The restored regression first failed because a valid stored image set the
omitted flag. With the descriptor restored, all 38 Chief timeline tests pass.
The test checks mixed content indices, stored and inline source types, private
payload omission, and malformed IDs. Strict runtime lint also passes.

The complete `chief/timeline/attachments.rs` file now matches the preserved
snapshot SHA-256 `776173bc88ea5dfd7e5374303613d8755c885956bda40cccf9894ce37071516f`.
This closes that file's inherited review. It does not prove signed desktop
acceptance or close other timeline files.
