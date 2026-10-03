import assert from "node:assert/strict"
import test from "node:test"
import { CompressedLocalStorageSaveRepository } from "./save-repository.ts"
import { parseSaveSlot } from "./save-schema.ts"
import { currentSaveFixture } from "./current-save-fixture.ts"

const mature = parseSaveSlot(currentSaveFixture())

test("browser save rejects an old generation after asynchronous compression", async () => {
  let stored = "previous valid save";
  let current = true;
  let release: (value: string) => void = () => { throw new Error("missing codec release"); };
  const compressed = new Promise<string>((resolve) => { release = resolve; });
  const repository = new CompressedLocalStorageSaveRepository({
    getItem: () => stored,
    setItem: (_key, value) => { stored = value; },
  }, "save", { encode: async () => compressed, decode: async (text) => text });
  const pending = repository.save(mature, () => current);
  current = false;
  release("new save");
  await pending;
  assert.equal(stored, "previous valid save");
});

test("compressed browser save repository preserves the current authority state", async () => {
  let stored: string | null = null
  const codec = {
    encode: async (text: string) => `encoded:${text}`,
    decode: async (text: string) => text.slice("encoded:".length),
  }
  const repository = new CompressedLocalStorageSaveRepository({
    getItem: () => stored,
    setItem: (_key, value) => { stored = value },
  }, "save", codec)

  await repository.save(mature)
  assert.equal(typeof stored, "string")
  assert.ok((stored ?? "").startsWith("gzip:"))
  assert.deepEqual(await repository.load(), mature)
})

test("compressed browser save repository rejects unwrapped local data", async () => {
  const repository = new CompressedLocalStorageSaveRepository({
    getItem: () => JSON.stringify(mature),
    setItem: () => undefined,
  })
  await assert.rejects(repository.load(), /gzip:/)
})

test("compressed browser save repository reports malformed JSON and rejects old schema", async () => {
  let stored = "gzip:{"
  const repository = new CompressedLocalStorageSaveRepository({
    getItem: () => stored,
    setItem: () => undefined,
  }, "save", { encode: async (text) => text, decode: async (text) => text })
  await assert.rejects(repository.load(), /存档不是合法 JSON/)
  stored = `gzip:${JSON.stringify({ ...mature, schema_version: 1 })}`
  await assert.rejects(repository.load(), /schema_version 1：不支持旧版本；仅支持 schema_version=3/)
  stored = `gzip:${JSON.stringify({ ...mature, seed: Number.MAX_SAFE_INTEGER + 1 })}`
  await assert.rejects(repository.load(), /seed/)
  const candle = mature.snapshot.daily_candles["600101"]?.[0]
  if (candle === undefined) throw new Error("current save fixture must contain a daily candle")
  const daily_candles = { ...mature.snapshot.daily_candles, "600101": [{ ...candle, trade_stats: { turnover_cents: 42, trade_count: 1 } }] }
  stored = `gzip:${JSON.stringify({ ...mature, snapshot: { ...mature.snapshot, daily_candles } })}`
  await assert.rejects(repository.load(), /turnover_cents/)
})

test("compressed browser save repository reports storage and decoding failures", async () => {
  const writeFailure = new CompressedLocalStorageSaveRepository({
    getItem: () => null,
    setItem: () => { throw new Error("quota exhausted") },
  }, "save", { encode: async (text) => text, decode: async (text) => text })
  await assert.rejects(writeFailure.save(mature), /写入浏览器存档失败.*quota exhausted/)

  const readFailure = new CompressedLocalStorageSaveRepository({
    getItem: () => { throw new Error("storage unavailable") },
    setItem: () => undefined,
  })
  await assert.rejects(readFailure.load(), /读取浏览器存档失败.*storage unavailable/)

  const decodeFailure = new CompressedLocalStorageSaveRepository({
    getItem: () => "gzip:broken",
    setItem: () => undefined,
  }, "save", { encode: async (text) => text, decode: async () => { throw new Error("bad gzip") } })
  await assert.rejects(decodeFailure.load(), /解压浏览器存档失败.*bad gzip/)
})
