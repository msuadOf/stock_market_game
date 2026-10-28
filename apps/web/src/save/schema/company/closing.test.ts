import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parseSaveJson } from "../../save-schema.ts";
import { materializeSaveValue } from "../primitives.ts";
import { parseRawSaveJson } from "../raw-json.ts";
import { parseClosingRegistry } from "./closing.ts";

const tuple = (source: unknown) => ({ versions: [], restatements: [[{ Standalone: 0 }, [[source, "2030-03"]]]] });

test("真实更正日结完整SaveSlot的restatement BusinessEventId数值无损消费", { timeout: 10000 }, () => {
  const bytes = readFileSync(new URL("../../fixtures/correction/rust-completed-save.json", import.meta.url));
  assert.equal(createHash("sha256").update(bytes).digest("hex"), "92127f8a150787c3d9ec64f44211117b1f1435027ed643bfa779d908dd35b6bd");
  const parsed = parseSaveJson(bytes.toString("utf8"));
  assert.deepEqual(parsed, JSON.parse(bytes.toString("utf8")));
  assert.ok(Object.keys(parsed.report_correction_operations).length > 0);
});

test("restatement source只接受Rust裸数wire的安全整数，保持number类型", { timeout: 10000 }, () => {
  for (const source of [0, 900000001, Number.MAX_SAFE_INTEGER]) {
    const value = tuple(source);
    assert.deepEqual(parseClosingRegistry(value), value);
    assert.deepEqual(materializeSaveValue(parseClosingRegistry(parseRawSaveJson(JSON.stringify(value)))), value);
  }
  for (const source of ["0", "900000001", "18446744073709551615", -0, -1, 0.5, Number.MAX_SAFE_INTEGER + 1, NaN, Infinity, null]) {
    assert.throws(() => parseClosingRegistry(tuple(source)), /restatements\[0\]\[1\]\[0\]\[0\]/);
  }
});

test("restatement raw source拒绝负零小数指数与超安全域，不能先舍入", { timeout: 10000 }, () => {
  const original = JSON.stringify(tuple(900000001));
  for (const token of ["-0", "900000001.00000000000000001", "900000001e0", "9007199254740993", "18446744073709551615"]) {
    const raw = original.replace("900000001", token);
    assert.throws(() => parseClosingRegistry(parseRawSaveJson(raw)), /restatements\[0\]\[1\]\[0\]\[0\]/);
  }
});
