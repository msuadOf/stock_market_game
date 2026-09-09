import { readFileSync } from "node:fs";
import assert from "node:assert/strict";
import { describe, it } from "node:test";

const appSource = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
const gridSource = readFileSync(new URL("./MarketGrid.tsx", import.meta.url), "utf8");

describe("局部刷新边界", () => {
  it("根 App 不订阅完整 snapshot", () => {
    assert.ok(!appSource.includes("const snapshot = useSelector"));
    assert.ok(!appSource.includes("snapshot={snapshot}"));
  });

  it("AG Grid 通过稳定行 id 和异步事务更新", () => {
    assert.ok(gridSource.includes("getRowId="));
    assert.ok(gridSource.includes("applyTransactionAsync"));
  });
});
