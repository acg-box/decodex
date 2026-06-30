# Radar

Radar is the Decodex auxiliary automation tool for upstream review queues,
release deltas, artifact validation, signal rendering, bundle generation, and
social publishing reservations.

Run it from the workspace with:

```sh
cargo run -p radar -- --help
```

Installed operators use the `radar` binary directly:

```sh
radar validate .agent/automations/decodex/cache/site-content/signals
```
