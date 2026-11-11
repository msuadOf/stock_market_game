import assert from "node:assert/strict"
import test from "node:test"
import { CompressedLocalStorageSaveRepository, gzipSaveCodec } from "./save-repository.ts"
import { gzipSync, gunzipSync } from "node:zlib"
import { parseSaveSlot } from "./save-schema.ts"
import { currentSaveFixture } from "./current-save-fixture.ts"

const mature = parseSaveSlot(currentSaveFixture())

test("真实 gzip 保留 Unicode、空内容与现有 base64 格式", { timeout: 10000 }, async () => {
  for (const text of ["", "中文存档😀\u0000\n金额：123.45，𠮷"] ) {
    const encoded = await gzipSaveCodec.encode(text)
    assert.match(encoded, /^[A-Za-z0-9+/]*={0,2}$/)
    assert.equal(gunzipSync(Buffer.from(encoded, "base64")).toString("utf8"), text)
    assert.equal(await gzipSaveCodec.decode(gzipSync(text).toString("base64")), text)
    assert.equal(await gzipSaveCodec.decode(encoded), text)
  }
})

test("真实 gzip 大内容往返且不按字符串 iterable 分配全部字节", { timeout: 10000 }, async () => {
  let seed = 42
  const fragments: string[] = []
  for (let index = 0; index < 16000; index++) {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0
    fragments.push(`${seed.toString(36)}：真实日终😀\n`)
  }
  const text = fragments.join("").repeat(3)
  const encoded = await gzipSaveCodec.encode(text)
  assert.ok(Buffer.from(encoded, "base64").byteLength > 32768, "fixture 必须真实跨越多个 base64 块")
  const original = Uint8Array.from
  Uint8Array.from = ((source: Iterable<number> | ArrayLike<number>, ...rest: unknown[]) => {
    assert.notEqual(typeof source, "string", "base64 字节应直接写入预分配缓冲区")
    return Reflect.apply(original, Uint8Array, [source, ...rest])
  }) as typeof Uint8Array.from
  try { assert.equal(await gzipSaveCodec.decode(encoded), text) }
  finally { Uint8Array.from = original }
})

test("真实 gzip 拒绝损坏 base64、非 gzip 和截断流且保留仓库错误前缀", { timeout: 10000 }, async () => {
  const encoded = await gzipSaveCodec.encode("有效日终")
  for (const corrupted of ["%%%", btoa("invalid gzip"), encoded.slice(0, -8)]) {
    await assert.rejects(gzipSaveCodec.decode(corrupted))
    const repository = new CompressedLocalStorageSaveRepository({ getItem: () => `gzip:${corrupted}`, setItem: () => undefined })
    await assert.rejects(repository.load(), /解压浏览器存档失败：/)
  }
})

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

test("普通读取保持可变，WASM 读取返回深冻结的同值存档", async () => {
  let stored: string | null = null
  const repository = new CompressedLocalStorageSaveRepository({
    getItem: () => stored,
    setItem: (_key, value) => { stored = value },
  }, "save", { encode: async (text) => text, decode: async (text) => text })
  await repository.save(mature)
  const ordinary = await repository.load()
  const frozen = await repository.loadFrozen()
  assert.ok(ordinary && frozen)
  assert.deepEqual(ordinary, frozen)
  assert.equal(Object.isFrozen(ordinary.snapshot), false)
  assert.equal(Object.isFrozen(frozen.snapshot), true)
  assert.equal(Object.isFrozen(frozen.snapshot.daily_candles), true)
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
  await assert.rejects(repository.load(), /schema_version/)
  stored = `gzip:${JSON.stringify({ ...mature, seed: Number.MAX_SAFE_INTEGER + 1 })}`
  await assert.rejects(repository.load(), /seed/)
  const candle = mature.snapshot.daily_candles["0"]?.[0]
  if (candle === undefined) throw new Error("current save fixture must contain a daily candle")
  const daily_candles = { ...mature.snapshot.daily_candles, "0": [{ ...candle, trade_stats: { turnover_cents: 42, trade_count: 1 } }] }
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
