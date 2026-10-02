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

test("App restores its initial archive before starting the memory session", () => {
  const source = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  const initialization = source.slice(source.indexOf("let ownedHost"), source.indexOf("// eslint-disable-next-line react-hooks/exhaustive-deps"));
  assert.ok(initialization.includes("initialSaveSourceRef.current.read("));
  assert.ok(initialization.includes("await host.load(initialSlot)"));
  assert.ok(initialization.indexOf("await host.load(initialSlot)") < initialization.indexOf("host.start("));
  assert.ok(initialization.includes("initialSaveSourceRef.current.complete()"));
});

test("App keeps archive loads outside tick updates and never projects live orders from an archive", () => {
  const source = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  const updates = source.slice(source.indexOf("onApplied(reduction"), source.indexOf("onFailure(failure)"));
  assert.equal(updates.includes(".load("), false);
  assert.equal(source.includes("projectPlayerOrders(slot)"), false);
  assert.ok(source.includes("validateDayEndArchive(slot)"));
});
