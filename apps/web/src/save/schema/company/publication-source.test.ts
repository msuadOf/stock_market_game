import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parsePublicLibrary } from "./reports.ts";

const saved = JSON.parse(readFileSync(new URL("../../fixtures/current-schema-save.json", import.meta.url), "utf8")) as { public_library: { next_seq: number; announcements: unknown[]; reports: Record<string, unknown>[] } };

test("Simple 与 Simulation 来源保留同一完整报告存档契约", () => {
  for (const source of ["SimpleGenerated", "SimulationAccounting"]) {
    const library = { ...saved.public_library, reports: saved.public_library.reports.map((report) => ({ ...report, source })) };
    assert.ok(library.reports.length > 0);
    assert.deepEqual(parsePublicLibrary(library), library);
  }
});

test("存档拒绝缺失或未知报告来源，不把旧内容默认为仿真", () => {
  for (const source of [undefined, null, "Unknown"]) {
    const reports = saved.public_library.reports.map((report) => ({ ...report, source }));
    assert.throws(() => parsePublicLibrary({ ...saved.public_library, reports }));
  }
  const reports = saved.public_library.reports.map((report) => {
    const missing = { ...report }; delete missing.source; return missing;
  });
  assert.throws(() => parsePublicLibrary({ ...saved.public_library, reports }), /source/);
});
