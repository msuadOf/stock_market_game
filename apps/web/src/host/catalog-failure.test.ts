import assert from "node:assert/strict";
import test from "node:test";
import { parseHostFailure } from "./protocol-failure.ts";

const failure = (fault: unknown) => ({ code: "CATALOG_BASELINE_FAILED", where: "server.actor.public_baseline", message: "不可信 wire 说明", context: { operation: "public_baseline", generation: "2", fault }, recoverable: false, recoveryActions: ["重新同步"] });

test("三个transport共用数值目录fault生成中文why并保留进度", () => {
  for (const where of ["server.actor.public_baseline", "web-wasm.company_catalog_view", "desktop.actor.restore"]) {
    const parsed = parseHostFailure({ ...failure({ code: 3019, context: [7] }), where });
    assert.match(parsed.message, /CompanyId 7/);
    assert.match(parsed.message, /StockCatalog/);
    assert.equal(parsed.where, where);
    assert.equal((parsed.context as { generation: string }).generation, "2");
  }
});

test("catalog fault 缺键、多键、错码及非单一u32上下文都拒绝", () => {
  for (const fault of [undefined, null, { code: 3019 }, { code: 3019, context: [7], extra: true }, { code: 3018, context: [7] }, ...[[], [7, 8], [-1], [0.5], [4294967296], ["7"], [7n]].map(context => ({ code: 3019, context }))]) {
    assert.throws(() => parseHostFailure(failure(fault)));
  }
});
