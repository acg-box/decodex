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

## Complete inherited reference review on 2026-09-27

This document is the current owner of the inherited stored-image-references.md
review. The original SHA-256 is preserved in the inherited-file register.
The original alpha.16 observations are historical. The current explicit native
qualification uses Codex CLI 0.158.0-alpha.2.1, SHA-256
3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a.

| Inherited review area | Retained owner and boundary |
| --- | --- |
| Inline reference refactor (1298) | Native ImageReference preserves inline image_url. Decodex consumes the wire representation, not internal Rust or Python SDK class names. |
| Internal upload and resolution (1305, 1425) | Native AttachmentStore owns preparation and upload. The fixed-cutoff app-server injects passthrough_image_store; InlineAttachmentStore returns inline upload bytes and NotFound for resolution. This is not a public image upload or byte-resolution RPC. |
| File references (1352) | Native input uses fileId; model/tool content uses file_id. Treat these as opaque references. Decodex preserves source type and original content index without exposing the private ID in projected history. |
| Prepared display history (1454) | Native history owns replacement of prepared image slots and preserves mixed file/inline order. Do not replace IDs with guessed backend URLs or use thread attachment associations as image storage. |
| Cold continuation (1425) | The restored native fixture tests both media-notification modes, complete live/cold history, reference order and detail in history, and one explicit continuation. Cold read causes no model request. Responses Lite removes detail hints on its model wire; display history retains them. |
| Image budgets and Guardian profiles (1431) | Native Codex owns image token estimates, reference-byte accounting, compaction and reviewer admission. Reference bytes do not measure remote image size. The synchronous profile is text-only; this test does not qualify the asynchronous image profile or its budget thresholds. |
| Remaining internal image paths | Native image-generation editing, prepared remote-store substitution and Node REPL evidence have separate owners and limits. This integration does not claim to qualify those paths. |

The fixed-cutoff source was rechecked at
codex-rs/app-server/src/message_processor.rs,
codex-rs/core/src/thread_manager.rs and
codex-rs/attachment-store/src/lib.rs. Installed experimental schemas retain
file references. ThreadAttachmentAddParams describes a thread-owned attachment
association with identityKey, attachmentType and payload; it does not define an
image byte resolver.

The complete inherited document is represented by this mapping and
[native Guardian evidence qualification](native-guardian-evidence-qualification.md).
Source-level claims about native internals remain separate from Decodex's tested
public consumers. The current media reader resolves supported inline or owned
local content; stored file references have no byte retrieval path here.

## Fresh native qualification

Two opt-in tests pass against the exact binary above in isolated temporary homes
with synthetic loopback model responses:

- installed_native_file_images_survive_cold_history: both notification modes
  retain file/inline/file order, exact live/cold display content and one explicit
  continuation. Each mode has one initial request and one continuation request.
- installed_guardian_preserves_native_image_profile_after_restart: cold resume
  makes no inference. Both saved file references remain in model input; the
  synchronous Guardian receives the original text restriction and no transcript
  image blocks. The synthetic review denies the command, with four model requests.

Logs: /tmp/decodex-image-reference-audit-native.log and
/tmp/decodex-image-reference-audit-guardian.log. No runtime source, dependency,
installed account, credential, approval policy or automation changes.

Opaque reference preservation and honest unsupported-media status are core
compatibility. Stored-image thumbnails or remote upload would be optional future
capabilities that need supported native APIs and a product decision. This closes
one inherited document review, not remote storage, asynchronous Guardian image
admission, tool-output image replay, image budget thresholds or signed desktop
presentation.
