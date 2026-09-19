# Settings visual system

General, Accounts, Diagnostics, and Chief defaults use the shell typography and
translucent materials. The shared settings group uses a 10 px radius, a light
border, and a 4/255 white tint. Content has a 780 px maximum width. Main labels
use 12.5 px; secondary labels use 10.5 px. Page headings use 15 px semibold.

General groups the two application preferences into compact rows. Switches show
the value; normal operation does not also show a badge and success notice.
Pending and failed changes remain visible, including failures when the previous
preference is still enabled. Chief defaults expand with the existing disclosure
animation and use labelled fields.

Accounts keeps usage and routing visible. Each account's management control opens
its reorder, profile, reauthentication, and logout actions. Only one account's
management section opens at a time. Opening another section clears the pending
logout confirmation. The existing confirmation and command authority remain in
place. Ordinary synchronized status does not add a permanent footer.

Diagnostics uses 38 px minimum rows and right-aligned states. The section heading
explains unprobed capabilities once. Actual failure details remain in their rows.
The global status surface continues to report connection errors.

Validation: GPUI suites passed 163 and 161 tests. Rendered captures covered all
three destinations. Strict Clippy and bundle signing were checked. This is a
presentation update; it does not change account bindings, stored preferences,
provider permissions, or authentication.

The final signed preview was opened as one instance. Native inspection verified
General, Accounts, Diagnostics, advanced Chief fields, and keyboard expansion and
collapse of account management. A native screenshot confirmed General's settled
layout and translucent groups. No account or operating-system preference was changed.
