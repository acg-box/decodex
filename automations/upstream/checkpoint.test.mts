import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { digest, saveCheckpoint } from "./checkpoint.mts";

const sha = (digit: string) => digit.repeat(40);
const json = (value: unknown) => `${JSON.stringify(value, null, 2)}\n`;

function fixture(run: (directory: string, save: (overrides?: Record<string, unknown>) => Record<string, unknown>) => void) {
  const directory = mkdtempSync(join(tmpdir(), "upstream-checkpoint-"));
  try {
    writeFileSync(join(directory, "state.json"), json({
      version: 1, last_reviewed_commit: sha("a"), reviewed_range_count: 10,
      unreviewed_count: 87, certified_review_ranges: [], pending_ci: [],
      active_run: { status: "active", fixed_target: sha("d"), owner_thread: "test-owner" },
      round_completion: { status: "partial_documentation_followup_remaining" },
    }));
    writeFileSync(join(directory, "inventory.json"), json(["b", "c", "d"].map((digit, index) => ({
      index: index + 1, commit: sha(digit), time: `2026-10-27T20:0${index}:00Z`,
    }))));
    writeFileSync(join(directory, "review.json"), json({ start_exclusive: sha("a"), records: [{
      index: 1, commit: sha("b"), classification: "native_owned", source_review_complete: true,
      behavior: "Preserve the retained consumer contract", decodex_consumer: "Existing local stdio adapter",
      implementation_symbols: ["native/adapter.rs::resume"], tests_executed: [],
      tests_read: [{ source: "native/tests.rs", symbol: "resume", revision: sha("d"), body_read: true }],
      validation_limits: ["Source review only; exact-artifact tests and delivery remain separate"],
    }] }));
    run(directory, (overrides = {}) => {
      const input = {
        expected_state_sha256: digest(readFileSync(join(directory, "state.json"))),
        review: "review.json", inventory: "inventory.json", observed_at: "2026-10-27T20:05:00Z",
        updates: { pending_ci: [{ pr: 7, head: sha("e") }] }, ...overrides,
      };
      writeFileSync(join(directory, "input.json"), json(input));
      return input;
    });
  } finally { rmSync(directory, { recursive: true, force: true }); }
}

test("publish cursor, certificate and remaining count together; preserve pending delivery", () => fixture((directory, input) => {
  input();
  const result = saveCheckpoint(directory, join(directory, "input.json"));
  assert.equal(result.cursor, sha("b"));
  assert.equal(result.remaining, 2);
  const state = JSON.parse(readFileSync(join(directory, "state.json"), "utf8"));
  assert.equal(state.reviewed_range_count, 11);
  assert.equal(state.next_unreviewed_commit, sha("c"));
  assert.equal(state.active_run.remaining_count, 2);
  assert.equal(state.pending_ci[0].pr, 7);
  assert.equal(state.certified_review_ranges[0].count, 1);
  const receipt = state.certified_review_ranges[0];
  assert.equal(digest(readFileSync(receipt.source_review_receipt)), receipt.source_review_sha256);
  const before = readFileSync(join(directory, "state.json"), "utf8");
  assert.equal(saveCheckpoint(directory, join(directory, "input.json")).replayed, true);
  assert.equal(readFileSync(join(directory, "state.json"), "utf8"), before);
}));

test("finish a fixed range without duplicating its certificate or review count", () => fixture((directory, input) => {
  const review = JSON.parse(readFileSync(join(directory, "review.json"), "utf8"));
  const first = review.records[0];
  review.records = ["b", "c", "d"].map((digit, index) => ({ ...first, index: index + 1, commit: sha(digit) }));
  writeFileSync(join(directory, "review.json"), json(review));
  input(); saveCheckpoint(directory, join(directory, "input.json"));
  input({ updates: { pending_ci: [] } }); saveCheckpoint(directory, join(directory, "input.json"));
  const state = JSON.parse(readFileSync(join(directory, "state.json"), "utf8"));
  assert.equal(state.last_reviewed_commit, sha("d"));
  assert.equal(state.unreviewed_count, 0);
  assert.equal(state.next_unreviewed_commit, null);
  assert.equal(state.oldest_unreviewed_commit_time, null);
  assert.equal(state.reviewed_range_count, 13);
  assert.equal(state.certified_review_ranges.length, 1);
}));

test("unread or out-of-order review leaves the current checkpoint unchanged", () => fixture((directory, input) => {
  const path = join(directory, "review.json");
  const review = JSON.parse(readFileSync(path, "utf8"));
  const before = readFileSync(join(directory, "state.json"), "utf8");
  review.records[0].tests_read[0].body_read = false;
  writeFileSync(path, json(review)); input();
  assert.throws(() => saveCheckpoint(directory, join(directory, "input.json")), /unread/);
  review.records[0].tests_read[0].body_read = true;
  review.records[0].commit = sha("c");
  writeFileSync(path, json(review)); input();
  assert.throws(() => saveCheckpoint(directory, join(directory, "input.json")), /non-contiguous/);
  assert.equal(readFileSync(join(directory, "state.json"), "utf8"), before);
  assert.equal(readdirSync(directory).includes("checkpoints"), false);
}));

test("stale input cannot overwrite a newer pending-work observation", () => fixture((directory, input) => {
  input();
  const path = join(directory, "state.json");
  const state = JSON.parse(readFileSync(path, "utf8")); state.pending_ci = [{ pr: 9 }];
  writeFileSync(path, json(state));
  assert.throws(() => saveCheckpoint(directory, join(directory, "input.json")), /State changed/);
  assert.equal(JSON.parse(readFileSync(path, "utf8")).pending_ci[0].pr, 9);
}));

test("changing an active fixed target or owner does not publish a checkpoint", () => fixture((directory, input) => {
  const path = join(directory, "inventory.json");
  const inventory = JSON.parse(readFileSync(path, "utf8")); inventory[2].commit = sha("e");
  writeFileSync(path, json(inventory)); input();
  assert.throws(() => saveCheckpoint(directory, join(directory, "input.json")), /fixed target/);
  inventory[2].commit = sha("d"); writeFileSync(path, json(inventory));
  input({ updates: { active_run: { owner_thread: "other-owner", status: "active" } } });
  assert.throws(() => saveCheckpoint(directory, join(directory, "input.json")), /different owner/);
}));

test("archive only explicit finished fields, with the complete original recoverable", () => fixture((directory, input) => {
  const original = readFileSync(join(directory, "state.json"), "utf8");
  input({ archive_fields: ["round_completion"] });
  saveCheckpoint(directory, join(directory, "input.json"), true);
  assert.equal(readFileSync(join(directory, "state.json"), "utf8"), original);
  assert.equal(readdirSync(directory).includes("archive"), false);
  saveCheckpoint(directory, join(directory, "input.json"));
  const state = JSON.parse(readFileSync(join(directory, "state.json"), "utf8"));
  assert.equal(state.round_completion, undefined);
  assert.equal(readFileSync(state.archived_state.path, "utf8"), original);
  assert.equal(digest(original), state.archived_state.sha256);
  input({ archive_fields: ["pending_ci"] });
  assert.throws(() => saveCheckpoint(directory, join(directory, "input.json")), /current field/);
}));

test("a pending-only save cannot write a caller-supplied cursor or counter", () => fixture((directory, input) => {
  input({ review: undefined, inventory: undefined, updates: { last_reviewed_commit: sha("d") } });
  assert.throws(() => saveCheckpoint(directory, join(directory, "input.json")), /derived/);
}));

test("CLI inspection is read-only and rejects unknown options", () => fixture((directory) => {
  const before = readFileSync(join(directory, "state.json"), "utf8");
  const script = new URL("./checkpoint.mts", import.meta.url);
  const shown = spawnSync(process.execPath, ["--experimental-strip-types", script.pathname, "--automation-dir", directory], { encoding: "utf8" });
  assert.equal(shown.status, 0, shown.stderr);
  assert.equal(JSON.parse(shown.stdout).cursor, sha("a"));
  assert.equal(readFileSync(join(directory, "state.json"), "utf8"), before);
  const invalid = spawnSync(process.execPath, [script.pathname, "--unknown"], { encoding: "utf8" });
  assert.equal(invalid.status, 1);
}));
