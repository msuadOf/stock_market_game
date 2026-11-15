import assert from "node:assert/strict";
import test from "node:test";
import { parseCpuSet } from "./libos-collect-performance.mjs";

test("验证 Linux 实际 affinity 而非宿主可用 CPU 数", () => {
  assert.deepEqual(parseCpuSet("Name:\tnode\nCpus_allowed_list:\t2-3,8\n"), [2, 3, 8]);
  for (const input of ["", "2", "5-2", "0-1024", "4-6,6-7", "3,2", "3,"]) {
    assert.throws(() => parseCpuSet("Cpus_allowed_list:\t" + input), /CPU/u);
  }
});
