# Loading feedback

## Rules

- Distinguish a first read, a refresh, an empty result, and a failed read.
- First workspace and conversation reads use a low-contrast chat skeleton with
  a right-aligned message shape and left-aligned reading lines. It reserves the
  reading area, breathes gently, and respects reduced motion. No dummy text is shown.
- Use a compact status only inside a small loading area. Use `ui_loading::loading`
  for first reads. It reserves 24 pixels, starts motion after 150 ms, and respects
  reduced motion. It stops when the loading row is removed. Do not force a
  minimum wait or display a fabricated percentage.
- Retain existing content during a refresh for the same source. Do not retain
  another account's or conversation's content under a new source label.
- A loading operation does not mean an empty account list or disabled preference.
- Use local button feedback for commands. Loading alone does not need a notification.
- Load earlier history without moving the reader's anchor. Do not fade or replay
  all saved messages when the read completes. Show live output as it arrives.
- Keep recovery, connection errors, and uncertain command outcomes explicit.
  An animation must not imply that a failed operation remains in progress.

## Source review, 2026-09-28

This inventory describes source inspection and targeted UI tests. It is not a
claim that every network delay was reproduced in the installed application.

| Surface | Decision |
| --- | --- |
| First workspace snapshot | Loading feedback replaces the premature welcome screen. |
| Main and native-agent conversation history | Shared feedback for the initial read; existing history stays visible on refresh. |
| Earlier local records | Shared feedback replaces the clickable load label while a read is pending. |
| Accounts | Show empty state only after a successful empty read; distinguish loading and unavailable. Retain existing rows during refresh. |
| Settings | Unknown persisted preferences show a small indicator, not an off switch. Local preferences remain usable. Save notifications stay in the existing notice system. |
| Model catalog | Reserve a small loading area; retain a valid current catalog during refresh. Keep unavailable separate. |
| Resources and inspection records | Shared local loading feedback. |
| Tool details, review details, and delivery records | Shared local loading feedback. |
| Tools and plugins | Shared first-read feedback; refresh keeps the same source's previous result visible. |
| Image preview | Shared feedback while the actual preview task is pending; failures retain their message. |
| Reply generation and cancellation | Keep the existing working/stop controls and native progress. Do not replace them with generic loading text. |
| Recap, installation, and authorization | Keep real stage feedback and the existing cancel/authorization actions. |
| Dictation and Live | Keep microphone connection/listening feedback in the composer. Do not create another loading panel. |
| Menu-bar account reads and Reset Card | Existing native ProgressView and button-local busy states remain; no global loading toast. |

## Checks

UI tests distinguish loading/empty/unavailable account states, check that unread
settings do not render off switches, and check that initial workspace loading does
not replace retained content during refresh. Existing history-anchor, preview,
recap, and recovery tests continue to apply.
