---
type: Reference
title: "Native integrations, automations and auxiliary tools"
description: "Native integrations, automations and auxiliary tools"
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-c03bc4468425d8e7887133da
    resource: repo://apps/decodex-publisher/src/lib.rs
  - id: openwiki-source-140806c6bf93492d2e78b1df
    resource: repo://apps/radar/src/lib.rs
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
  - id: openwiki-source-e9d609e612bb7e44111ec4b1
    resource: repo://crates/decodex-runtime/src/agent_integrations.rs
generated: { by: "codex", at: "2026-09-29T06:24:17.023Z" }
verified:
  - by: openwiki/0.6.0
    at: 2026-09-29T06:24:17.023Z
---


# Native integrations, automations and auxiliary tools

## Agent integrations

The service reads MCP status and installed plugins independently from native app-server. It reads the selected thread before and after the query and requires the same absolute working directory. Unsupported, unavailable and capacity-exceeded results remain distinct. A failed optional inventory does not prove that no tools are installed.

Plugin installation and connection configuration belong to Codex. Decodex retains read-only observations, native forms, resource links and task references. Inventory is not installation authority. UI clients do not copy credentials, directly edit native configuration, or substitute host files for account-owned state.

## Automation boundary

`automations/portfolio.toml` defines three managed native roles: the active upstream Maintainer, Content Manager and Xurl Publisher. Desired configuration does not prove host registration or execution. The retired upstream Reviewer and Health roles are not active requirements. Agent follow-up events are a separate service behavior.

```sh
python3 automations/decodex/scripts/config/render_automation_plan.py --json
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo make test-automations
```

Do not assume an OpenWiki scheduled workflow exists from an old setup comment. See [Wiki maintenance](../operations/wiki-maintenance.md).

## Auxiliary products

Radar owns bounded research artifacts, source bundles and local evidence retention. Publisher owns candidate records, reservations, xurl effects, budgets and readback. Neither owns the application database, Agent threads, or account selection.

The retained static-site source is outside the current upstream-maintenance scope; the site is retired pending a separate redesign. Its source presence does not imply a live publication obligation. Local caches under `.agent/automations/`, build outputs, account state and credentials are not public site inputs.

`Decodex.app` is the sole GUI. Its Swift menu bar and native client library are in-process components; the local helper is `decodex serve`. See [Runtime architecture](../architecture/runtime-architecture.md), [Radar and Publisher contracts](radar-publisher-contracts.md), and [Subscription voice](subscription-voice.md).

For tool observations, native forms and the retired local management boundary, use [Tools and connections](tools-plugins-and-apps.md).
