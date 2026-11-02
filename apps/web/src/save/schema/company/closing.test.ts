import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parseSaveJson } from "../../save-schema.ts";
import { materializeSaveValue } from "../primitives.ts";
import { parseRawSaveJson } from "../raw-json.ts";
import { parseClosingRegistry } from "./closing.ts";

const tuple = (source: unknown) => ({ versions: [], restatements: [[{ Standalone: 0 }, [[source, "2030-03"]]]] });

const currentCompletedSaveUrl = new URL("../../../../../../packages/engine/tests/fixtures/correction/current-completed-save.json", import.meta.url);
const historicalCompletedSaveUrl = new URL("../../fixtures/correction/rust-completed-save.json", import.meta.url);

const readCompletedSaveText = () => readFileSync(currentCompletedSaveUrl, "utf8");

test("历史completed SaveSlot保留原件，当前契约显式拒绝缺少runtime_sync_policy", { timeout: 10000 }, () => {
  const bytes = readFileSync(historicalCompletedSaveUrl);
  assert.equal(createHash("sha256").update(bytes).digest("hex"), "92127f8a150787c3d9ec64f44211117b1f1435027ed643bfa779d908dd35b6bd");
  const text = bytes.toString("utf8");
  assert.throws(() => parseSaveJson(text), /setup\.runtime_sync_policy/);
});

test("当前Rust producer生成的更正日结完整SaveSlot保持raw无损消费", { timeout: 10000 }, () => {
  const bytes = readFileSync(currentCompletedSaveUrl);
  assert.equal(createHash("sha256").update(bytes).digest("hex"), "88db89bb78a2599fbb9f1caf1407d06e6d0a3b2719ea35e72dbc943a49a3176c");
  const text = bytes.toString("utf8");
  const parsed = parseSaveJson(text);
  assert.deepEqual(parsed, JSON.parse(text));
  assert.equal(parsed.setup.runtime_sync_policy, "every_tick");
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

test("完整SaveSlot必须严格保留已完成报表更正操作", { timeout: 10000 }, () => {
  const text = readCompletedSaveText();
  const fixture = JSON.parse(text) as { report_correction_operations: Record<string, unknown> };
  const parsed = parseSaveJson(text);
  assert.deepEqual(parsed.report_correction_operations, fixture.report_correction_operations);
  assert.equal(parsed.report_correction_operations["session-correction-1"]?.request.operation_id, "session-correction-1");
});

test("完整SaveSlot拒绝缺失更正表、未知字段与更正身份不一致", { timeout: 10000 }, () => {
  const text = readCompletedSaveText();
  const save = JSON.parse(text) as Record<string, unknown>;
  const withoutOperations = { ...save };
  delete withoutOperations.report_correction_operations;
  assert.throws(() => parseSaveJson(JSON.stringify(withoutOperations)), /report_correction_operations/);
  const operations = save.report_correction_operations as Record<string, Record<string, unknown>>;
  const completed = operations["session-correction-1"]!;
  const request = completed.request as Record<string, unknown>;
  const withUnknownField = { ...save, report_correction_operations: { ...operations, "session-correction-1": { ...completed, request: { ...request, unexpected: true } } } };
  assert.throws(() => parseSaveJson(JSON.stringify(withUnknownField)), /report_correction_operations/);
  const withMismatchedIdentity = { ...save, report_correction_operations: { ...operations, "other-id": completed } };
  delete (withMismatchedIdentity.report_correction_operations as Record<string, unknown>)["session-correction-1"];
  assert.throws(() => parseSaveJson(JSON.stringify(withMismatchedIdentity)), /report_correction_operations/);
});

test("已完成更正操作的BusinessEventId在raw JSON中必须无损且为规范安全整数", { timeout: 10000 }, () => {
  const text = readCompletedSaveText();
  const start = text.indexOf('"report_correction_operations":');
  const end = text.indexOf('"public_library":', start);
  assert.ok(start >= 0 && end > start);
  const operations = text.slice(start, end);
  for (const token of ["-0", "900000001.0", "900000001e0", "9007199254740993"]) {
    const corruptedOperations = operations.replace('"source":900000001', `"source":${token}`);
    assert.notEqual(corruptedOperations, operations);
    assert.throws(() => parseSaveJson(`${text.slice(0, start)}${corruptedOperations}${text.slice(end)}`), /report_correction_operations/);
  }
});


type MutableCorrectionSave = {
  report_correction_operations: Record<string, {
    request: { entries: Array<{ date: string; lines: Array<{ amount: string }> }> };
    committed_at: { date: string };
  }>;
};

test("更正金额codec与host一致：保留Rust可解语法并约束i128分范围", { timeout: 10000 }, () => {
  const text = readCompletedSaveText();
  const save = JSON.parse(text) as MutableCorrectionSave;
  const operation = save.report_correction_operations["session-correction-1"];
  const line = operation.request.entries[0].lines[0];
  for (const amount of ["10", "+10.0", "\u008510.00\u0085", "1701411834604692317316873037158841057.27"]) {
    line.amount = amount;
    assert.equal(parseSaveJson(JSON.stringify(save)).report_correction_operations["session-correction-1"]?.request.entries[0]?.lines[0]?.amount, amount);
  }
  for (const amount of ["1701411834604692317316873037158841057.28", "-1701411834604692317316873037158841057.28"]) {
    line.amount = amount;
    assert.throws(() => parseSaveJson(JSON.stringify(save)), /report_correction_operations/);
  }
});

test("更正请求及完成时间的CivilDate遵守1900至2199算法窗", { timeout: 10000 }, () => {
  const text = readCompletedSaveText();
  const original = JSON.parse(text) as MutableCorrectionSave;
  const validYears = ["1900-01-01", "2199-12-31"];
  for (const date of validYears) {
    const save = structuredClone(original);
    const operation = save.report_correction_operations["session-correction-1"];
    operation.request.entries[0].date = date;
    operation.committed_at.date = date;
    assert.doesNotThrow(() => parseSaveJson(JSON.stringify(save)));
  }
  for (const date of ["1899-12-31", "2200-01-01", "0001-01-01"]) {
    for (const field of ["request", "committed_at"]) {
      const save = structuredClone(original);
      const operation = save.report_correction_operations["session-correction-1"];
      if (field === "request") operation.request.entries[0].date = date;
      else operation.committed_at.date = date;
      assert.throws(() => parseSaveJson(JSON.stringify(save)), /report_correction_operations/);
    }
  }
});
