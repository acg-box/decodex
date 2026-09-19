# Chief workspace integration

## Scope

Integrate the 49 Chief workspace commits with upstream main `cc44cda4d`.
Keep native paginated history, provider observations, capacity retry and cancellation,
recursive managers, per-message execution settings, attachments, usage, and voice.
The exact-current protocol is 2.26. Older clients must reconnect with a matching build.

## Database upgrade paths

The SQLite owner remains `database/`. SQL migration contents and existing ledger
rows are unchanged. New databases use the local 1–22 sequence, then observation
indexes and capacity retry at 23–24. Databases that already have the upstream
observation migration at 15 retain upstream 15–16, then apply the local additions
at 17–24. Both paths finish at version 24 with the same verified schema inventory.
Selection uses the recorded migration name and still validates every SQL digest.
Unknown or modified histories remain rejected.

Tests reconstruct every version from 14 through 24 in both sequences, preserve the
existing version/name/digest rows, verify final schema parity, and reopen twice.
No user database was reset or changed to resolve the merge.

## Interaction and history

Keep the current continuous Markdown UI. Add capacity cancellation to this UI instead
of restoring the old history cards. Preserve terminal failure text and deduplicate
asynchronous questions against terminal readback. Provider usage observations and
the structured composer/turn usage projection both receive native notifications.

## Limits

The latest physical scrolling and navigation acceptance remains pending. The composer
uses a non-transparent material; true local backdrop blur is not implemented. Existing
opt-in real subscription voice tests are not rerun without their qualification setup.
