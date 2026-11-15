import assert from "node:assert/strict";
import { test } from "node:test";
import { readQuickArchive } from "./quick-archive-read.ts";

test("Browser 读快速槽前等权威存档围栏，不写入任何内容", async () => {
  const order: string[] = [];
  const value = await readQuickArchive({
    fence: async () => { order.push("fence"); },
    readLegacy: async () => { order.push("read"); return "browser"; },
    validate: (slot) => slot,
  });
  assert.equal(value, "browser");
  assert.deepEqual(order, ["fence", "read"]);
});

test("仅明确无新槽时读旧槽，I/O 错误或不合法数据不回退", async () => {
  let legacyReads = 0;
  const base = {
    readLegacy: async () => { legacyReads++; return "旧槽"; },
    validate: (value: unknown) => {
      if (typeof value !== "string") throw new Error("存档格式无效");
      return value;
    },
  };
  assert.equal(await readQuickArchive({ ...base, readAuthority: async () => "SQLite" }), "SQLite");
  assert.equal(await readQuickArchive({ ...base, readAuthority: async () => null }), "旧槽");
  await assert.rejects(readQuickArchive({ ...base, readAuthority: async () => { throw new Error("I/O 失败"); } }), /I\/O 失败/);
  await assert.rejects(readQuickArchive({ ...base, readAuthority: async () => undefined }), /存档格式无效/);
  assert.equal(legacyReads, 1);
});
