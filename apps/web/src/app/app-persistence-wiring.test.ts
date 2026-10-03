import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("App queries current player orders without generating persistence", () => {
  const source = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  const trading = readFileSync(new URL("./useTradingCommands.ts", import.meta.url), "utf8");
  const refresh = trading.slice(trading.indexOf("const refreshPlayerOrders"), trading.indexOf("// 自动单添加"));
  assert.ok(source.includes("useTradingCommands({ hostRef, playerOrderRefreshGateRef"));
  assert.ok(refresh.includes("host.playerWorkingOrders()"));
  assert.equal(refresh.includes("host.save()"), false);
});

test("App save controls schedule day-end persistence rather than writing intraday", () => {
  const source = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  const commands = readFileSync(new URL("./useSaveCommands.ts", import.meta.url), "utf8");
  const save = commands.slice(commands.indexOf("async function handleSave()"), commands.indexOf("async function handleLoad()"));
  const file = commands.slice(commands.indexOf("async function handleSaveFile()"), commands.indexOf("async function handleLoadFile()"));
  assert.ok(source.includes("useSaveCommands({ hostRef, initialSaveSourceRef, dayEndPersistenceRef"));
  assert.equal(save.includes(".save("), false);
  assert.equal(file.includes(".save("), false);
  assert.ok(file.includes("selectDayEndFileTarget()"));
  assert.ok(source.includes("dayEndPersistenceRef.current.completed("));
});

test("App restores its initial archive before starting the memory session", () => {
  const source = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  const lifecycle = readFileSync(new URL("./useSessionHostLifecycle.ts", import.meta.url), "utf8");
  const initialization = lifecycle.slice(lifecycle.indexOf("let ownedHost"), lifecycle.indexOf("function createSessionHost("));
  assert.ok(source.includes("useSessionHostLifecycle({ hostRef, initialSaveSourceRef, dayEndPersistenceRef"));
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
  const commands = readFileSync(new URL("./useSaveCommands.ts", import.meta.url), "utf8");
  assert.ok(commands.includes("validateDayEndArchive(slot)"));
  assert.equal(commands.includes("projectPlayerOrders(slot)"), false);
});
