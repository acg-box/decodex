# Task usage estimate lifetime

The desktop estimate panel now belongs to the current runtime source and native
thread binding. Clear the panel when its source, thread, selected task, or service
changes, or when the snapshot becomes unavailable. A request also captures a panel
epoch. A late result cannot reopen a closed panel or replace a newer request.

This delivers the inherited UI cache fix associated with upstream aee8a55ab601 at
fixed cutoff 595cc91e8cbb1c2ca822d0311dcf12709410c582. It retains the existing
runtime account/usage/read adapter and its process/account/history checks. It does
not add polling, pricing, quota conversion, or a new usage calculation.

The existing explicit query, exact integer micros, observation time, and separate
provider groups remain unchanged. Focused UI tests cover changed sources, changed
threads, unavailable snapshots, task switching, and late close/reopen responses.
These tests do not verify live billing values or signed desktop account switching.
