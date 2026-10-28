import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

type ObjectValue = Record<string, unknown>;

const listFields = new Set([
  "pending_prs", "pending_ci", "pending_adaptations", "pending_native_qualification",
  "pending_cleanup", "pending_security_followups", "optional_proposals",
  "baseline_audit_queue", "deferred_work",
]);
const objectFields = new Set([
  "active_run", "runtime_qualification", "continuation", "definition_update", "followups",
]);
const nullableFields = new Set(["pending_wiki_lifecycle"]);
const stringFields = new Set(["last_run_receipt", "current_qualification_receipt"]);
const derivedFields = new Set([
  "version", "updated_at",
  "last_reviewed_commit", "reviewed_range_start_exclusive", "reviewed_range_count",
  "last_observed_upstream_main", "upstream_observed_at", "upstream_main_commit_timestamp",
  "unreviewed_count", "next_unreviewed_commit", "oldest_unreviewed_commit_time",
  "certified_review_ranges", "current_review_receipts", "last_checkpoint", "archived_state",
]);

function object(value: unknown, label: string): ObjectValue {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(`${label} must be an object`);
  return value as ObjectValue;
}

function array(value: unknown, label: string): unknown[] {
  if (!Array.isArray(value)) throw new Error(`${label} must be an array`);
  return value;
}

function text(value: unknown, label: string): string {
  if (typeof value !== "string" || !value.trim()) throw new Error(`${label} must be nonempty text`);
  return value;
}

function commit(value: unknown, label: string): string {
  const result = text(value, label);
  if (!/^[a-f0-9]{40}$/.test(result)) throw new Error(`${label} must be a full commit SHA`);
  return result;
}

export function digest(value: string | Buffer): string {
  return createHash("sha256").update(value).digest("hex");
}

function encode(value: unknown): string {
  return `${JSON.stringify(value, null, 2)}\n`;
}

export function summary(state: ObjectValue, bytes: string): ObjectValue {
  return {
    state_sha256: digest(bytes),
    cursor: state.last_reviewed_commit,
    reviewed_total: state.reviewed_range_count,
    fixed_target: state.last_observed_upstream_main,
    remaining: state.unreviewed_count,
    next_commit: state.next_unreviewed_commit,
    certified_ranges: Array.isArray(state.certified_review_ranges) ? state.certified_review_ranges.length : 0,
    pending: Object.fromEntries([...listFields].map(key => [key, Array.isArray(state[key]) ? state[key].length : 0])),
    active_run_status: state.active_run ? object(state.active_run, "active_run").status : null,
  };
}

export function saveCheckpoint(directory: string, inputPath: string, dryRun = false): ObjectValue {
  const statePath = join(directory, "state.json");
  const original = readFileSync(statePath, "utf8");
  const state = object(JSON.parse(original), "state");
  const input = object(JSON.parse(readFileSync(inputPath, "utf8")), "checkpoint input");
  const allowed = new Set(["expected_state_sha256", "updates", "archive_fields", "review", "inventory", "observed_at"]);
  for (const key of Object.keys(input)) if (!allowed.has(key)) throw new Error(`Unknown checkpoint input: ${key}`);
  const source = (key: string) => {
    const path = resolve(dirname(inputPath), text(input[key], key));
    return { path, bytes: readFileSync(path, "utf8") };
  };
  if (Boolean(input.review) !== Boolean(input.inventory)) throw new Error("review and inventory must be supplied together");
  const reviewFile = input.review ? source("review") : null;
  const inventoryFile = input.inventory ? source("inventory") : null;
  const inputHash = digest(encode({ input, review: reviewFile?.bytes, inventory: inventoryFile?.bytes }));
  if (state.last_checkpoint && object(state.last_checkpoint, "last_checkpoint").input_sha256 === inputHash) {
    return { ...summary(state, original), replayed: true };
  }
  if (input.expected_state_sha256 !== digest(original)) throw new Error("State changed; read the current checkpoint summary before saving");

  const next = structuredClone(state);
  const updates = object(input.updates ?? {}, "updates");
  for (const [key, value] of Object.entries(updates)) {
    if (listFields.has(key)) array(value, key);
    else if (objectFields.has(key)) object(value, key);
    else if (nullableFields.has(key)) { if (value !== null) object(value, key); }
    else if (stringFields.has(key)) text(value, key);
    else throw new Error(`Unsupported update field: ${key}; review counters are derived`);
    next[key] = value;
  }
  if (state.active_run && object(state.active_run, "active_run").status === "active" && next.active_run) {
    if (object(next.active_run, "active_run").owner_thread !== object(state.active_run, "active_run").owner_thread) throw new Error("The active run has a different owner");
  }

  const files: Array<{ path: string; bytes: string }> = [];
  const now = new Date().toISOString();
  const archived = array(input.archive_fields ?? [], "archive_fields").map(key => text(key, "archive field"));
  if (archived.length) {
    for (const key of archived) {
      if (key.startsWith("pending_") || derivedFields.has(key) || listFields.has(key) || objectFields.has(key) || nullableFields.has(key) || stringFields.has(key)) {
        throw new Error(`Cannot archive current field: ${key}`);
      }
      const value = state[key];
      if (value && typeof value === "object" && !Array.isArray(value) && object(value, key).active === true) {
        throw new Error(`Cannot archive active owner: ${key}`);
      }
    }
    const path = join(directory, "archive", `state-${digest(original)}.json`);
    files.push({ path, bytes: original });
    next.archived_state = { path, sha256: digest(original), fields: archived };
    for (const key of archived) delete next[key];
  }

  if (reviewFile && inventoryFile) {
    const review = object(JSON.parse(reviewFile.bytes), "review");
    const inventory = array(JSON.parse(inventoryFile.bytes), "inventory").map((value, index) => {
      const entry = object(value, "inventory entry");
      return { commit: commit(entry.commit, "inventory commit"), time: text(entry.time, "commit time"), index: index + 1 };
    });
    if (new Set(inventory.map(entry => entry.commit)).size !== inventory.length) throw new Error("Inventory contains duplicate commits");
    const start = commit(review.start_exclusive, "review start");
    const target = inventory.at(-1)?.commit ?? start;
    if (target !== state.last_observed_upstream_main && !input.observed_at) throw new Error("A new fixed target needs its observation time");
    if (state.active_run && object(state.active_run, "active_run").status === "active") {
      const owner = object(state.active_run, "active_run");
      if (owner.fixed_target !== target) throw new Error("The active run has a different fixed target");
      if (next.active_run && object(next.active_run, "active_run").owner_thread !== owner.owner_thread) throw new Error("The active run has a different owner");
    }
    const records = array(review.records, "review records");
    if (records.length > inventory.length) throw new Error("Review exceeds the fixed inventory");
    records.forEach((value, index) => {
      const record = object(value, "review record");
      if (record.index !== index + 1 || record.commit !== inventory[index]?.commit || record.source_review_complete !== true) {
        throw new Error(`Incomplete or non-contiguous source review at index ${index + 1}`);
      }
      text(record.behavior, "changed behavior");
      text(record.decodex_consumer, "Decodex consumer or exclusion evidence");
      if (!["already_covered", "native_owned", "not_applicable", "necessary_repair", "optional_decision", "deferred"].includes(String(record.classification))) throw new Error("Missing review classification");
      if (!array(record.implementation_symbols, "implementation symbols").length) throw new Error("Missing implementation evidence");
      array(record.tests_executed, "tests executed");
      array(record.validation_limits, "validation limits");
      for (const value of array(record.tests_read, "tests read")) {
        const test = object(value, "test read");
        text(test.source, "test source"); text(test.symbol, "test symbol"); text(test.revision, "test revision");
        if (test.body_read !== true) throw new Error("A relevant test body remains unread");
      }
    });
    const cursor = commit(state.last_reviewed_commit, "current cursor");
    const previousIndex = [start, ...inventory.map(entry => entry.commit)].indexOf(cursor);
    if (previousIndex < 0 || previousIndex > records.length) throw new Error("Review cannot skip or move behind the current cursor");
    const total = state.reviewed_range_count;
    if (!Number.isSafeInteger(total) || Number(total) < 0) throw new Error("Invalid certified review total");
    const snapshot = join(directory, "checkpoints", `review-${digest(reviewFile.bytes)}.json`);
    files.push({ path: snapshot, bytes: reviewFile.bytes });
    const end = inventory[records.length - 1]?.commit ?? start;
    const ranges = array(state.certified_review_ranges ?? [], "certified ranges");
    next.certified_review_ranges = records.length === previousIndex ? ranges : [...ranges, {
      start_exclusive: cursor, end_inclusive: end, count: records.length - previousIndex,
      source_review_receipt: snapshot, source_review_sha256: digest(reviewFile.bytes), reviewed_at: now,
      limits: "Source review only. Runtime qualification, delivery and installed acceptance remain separate.",
    }];
    next.last_reviewed_commit = end;
    if (records.length > previousIndex) next.last_reviewed_at = now;
    next.reviewed_range_count = Number(total) + records.length - previousIndex;
    next.last_observed_upstream_main = inventory.at(-1)?.commit ?? start;
    next.upstream_main_commit_timestamp = inventory.at(-1)?.time ?? null;
    next.upstream_observed_at = text(input.observed_at ?? state.upstream_observed_at, "fixed target observation time");
    next.unreviewed_count = inventory.length - records.length;
    next.next_unreviewed_commit = inventory[records.length]?.commit ?? null;
    next.oldest_unreviewed_commit_time = inventory[records.length]?.time ?? null;
    next.current_review_receipts = [snapshot];
    if (next.active_run) Object.assign(object(next.active_run, "active_run"), {
      fixed_target: next.last_observed_upstream_main, reviewed_count: records.length,
      source_review_complete_count: records.length, remaining_count: next.unreviewed_count,
    });
  }

  next.updated_at = now;
  next.last_checkpoint = { input_sha256: inputHash, saved_at: now };
  const bytes = encode(next);
  if (!dryRun) {
    for (const file of files) {
      mkdirSync(dirname(file.path), { recursive: true });
      if (!existsSync(file.path)) writeFileSync(file.path, file.bytes, { flag: "wx", mode: 0o600, flush: true });
      else if (readFileSync(file.path, "utf8") !== file.bytes) throw new Error("Immutable receipt differs from its digest");
    }
    if (readFileSync(statePath, "utf8") !== original) throw new Error("State changed during preparation; checkpoint not published");
    const temporary = `${statePath}.${process.pid}.tmp`;
    try {
      writeFileSync(temporary, bytes, { flag: "wx", mode: 0o600, flush: true });
      renameSync(temporary, statePath);
    } finally {
      if (existsSync(temporary)) unlinkSync(temporary);
    }
  }
  return { ...summary(next, bytes), dry_run: dryRun, archived_fields: archived };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const { values } = parseArgs({ options: { "automation-dir": { type: "string" }, input: { type: "string" }, "dry-run": { type: "boolean" } } });
    const directory = values["automation-dir"];
    if (!directory) throw new Error("Usage: node --experimental-strip-types automations/upstream/checkpoint.mts --automation-dir DIR [--input FILE] [--dry-run]");
    const input = values.input;
    const bytes = readFileSync(join(directory, "state.json"), "utf8");
    console.log(JSON.stringify(input ? saveCheckpoint(directory, resolve(input), values["dry-run"] ?? false) : summary(object(JSON.parse(bytes), "state"), bytes)));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
