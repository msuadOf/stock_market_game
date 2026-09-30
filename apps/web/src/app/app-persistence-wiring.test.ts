import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("App queries current player orders without generating persistence", () => {
  const source = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  const refresh = source.slice(source.indexOf("const refreshPlayerOrders"), source.indexOf("// 自动单添加"));
  assert.ok(refresh.includes("host.playerWorkingOrders()"));
  assert.equal(refresh.includes("host.save()"), false);
});

test("App save controls schedule day-end persistence rather than writing intraday", () => {
  const source = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  const save = source.slice(source.indexOf("async function handleSave()"), source.indexOf("async function handleLoad()"));
  const file = source.slice(source.indexOf("async function handleSaveFile()"), source.indexOf("async function handleLoadFile()"));
  assert.equal(save.includes(".save("), false);
  assert.equal(file.includes(".save("), false);
  assert.ok(file.includes("selectDayEndFileTarget()"));
  assert.ok(source.includes("dayEndPersistenceRef.current.completed("));
});
