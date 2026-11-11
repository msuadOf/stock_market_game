import assert from "node:assert/strict"
import test from "node:test"
import { scanArchiveRoot } from "./archive-root-scan.ts"
import { parseRawSaveJson } from "./raw-json.ts"
import { RawSaveNumber } from "./primitives.ts"

async function* split(text: string, width: number): AsyncIterable<string> {
  for (let cursor = 0; cursor < text.length; cursor += width) yield text.slice(cursor, cursor + width)
}

test("扫描只保留白名单根元数据及枚举键，不保留全部账户值", { timeout: 10000 }, async () => {
  const accounts = Object.fromEntries(Array.from({ length: 2000 }, (_, index) => [String(index), { nested: [{ ignored: "真实个人数据", n: index }] }]))
  const text = JSON.stringify({ setup: { selected: 1 }, seed: "42", snapshot: { seq: 7, tick: 8, accounts }, history_reads: { "10": {}, "2": {}, "01": {} }, unknown: accounts })
  const result = await scanArchiveRoot(split(text, 16384))
  assert.deepEqual(result.rootKeys, ["setup", "seed", "snapshot", "history_reads", "unknown"])
  assert.deepEqual(result.rawMetadata, { setup: { selected: new RawSaveNumber("1") }, seed: "42", "snapshot.seq": new RawSaveNumber("7"), "snapshot.tick": new RawSaveNumber("8") })
  assert.equal(result.keySets["snapshot.accounts"]?.length, 2000)
  assert.deepEqual(result.keySets.history_reads, ["2", "10", "01"])
  assert.equal(result.capturedContainers, 1, "所有非元数据账户容器必须被丢弃")
  assert.equal(result.schemaValidated, false)
})

test("选中与跳过分支均保留旧词法首错和所有切分边界", { timeout: 10000 }, async () => {
  const inputs = ['{"unknown":{"x":1,"x":2}}', '{"setup":{"x":1,"x":2}}', '{"seed":"\\ud800"}', '{"unknown":"\\ud800"}', '{"snapshot":{"seq":1,}}', '[nullx]', 'truex', '{"setup":', '{"__proto__":1,"__proto__":2}']
  for (const text of inputs) {
    let expected: Error | undefined
    try { parseRawSaveJson(text) } catch (error) { assert.ok(error instanceof Error); expected = error }
    assert.ok(expected)
    for (let width = 1; width <= text.length; width++) {
      await assert.rejects(scanArchiveRoot(split(text, width)), error => error instanceof Error && error.name === expected!.name && error.message === expected!.message)
    }
  }
})

test("跨 chunk surrogate 计数等于整体 UTF-8，原始数字 token 不被提前舍入", { timeout: 10000 }, async () => {
  const text = '{"seed":"😀中文","receipt_watermark":9007199254740993}'
  for (const width of [1, 2, 5]) {
    const result = await scanArchiveRoot(split(text, width))
    assert.equal(result.utf8Bytes, Buffer.byteLength(text))
    assert.deepEqual(result.rawMetadata.receipt_watermark, new RawSaveNumber("9007199254740993"))
  }
})

test("点号 root 键不能冒充嵌套路径，非对象根仍只报告词法事实", { timeout: 10000 }, async () => {
  const result = await scanArchiveRoot(split('{"snapshot.accounts":{"fake":1},"runtime_state.strategy_states":{"fake":1},"snapshot":{"accounts":{"2":{}}}}', 1))
  assert.deepEqual(result.keySets, { "snapshot.accounts": ["2"] })
  for (const [text, kind] of [["[]", "array"], ['"x"', "string"], ["null", "null"], ["true", "boolean"], ["1.0", "number"]]) {
    const scan = await scanArchiveRoot(split(text!, 1))
    assert.equal(scan.rootKind, kind)
    assert.deepEqual(scan.rootKeys, [])
    assert.deepEqual(scan.rawMetadata, {})
    assert.equal(scan.schemaValidated, false)
  }
})

test("选择投影与旧 parser 的完整原值一致，确定性坏语料保留错误类型及路径", { timeout: 10000 }, async () => {
  const valid = '{"setup":{"n":1},"seed":"中文😀","snapshot":{"seq":3,"tick":4,"markets":{"0":{"last_price":"100"}},"accounts":{"4294967295":{},"2":{},"__proto__":{}}},"civil_clock":{"raw":true},"runtime_state":{"strategy_states":{"1":{}},"book_public_cutoff_sequences":{"0":"1"}},"receipt_watermark":1e0}'
  const raw = parseRawSaveJson(valid) as Record<string, unknown>
  const snapshot = raw.snapshot as Record<string, unknown>
  const runtime = raw.runtime_state as Record<string, unknown>
  const result = await scanArchiveRoot(split(valid, 3))
  assert.deepEqual(result.rawMetadata, { setup: raw.setup, seed: raw.seed, "snapshot.seq": snapshot.seq, "snapshot.tick": snapshot.tick, "snapshot.markets": snapshot.markets, civil_clock: raw.civil_clock, "runtime_state.book_public_cutoff_sequences": runtime.book_public_cutoff_sequences, receipt_watermark: raw.receipt_watermark })
  assert.deepEqual(result.keySets, { "snapshot.accounts": Object.keys(snapshot.accounts as object), "runtime_state.strategy_states": ["1"] })

  const inputs = new Set<string>()
  for (let index = 0; index < valid.length; index += 3) inputs.add(valid.slice(0, index))
  let seed = 42
  const alphabet = '{}[]:,"\\nulltruefalse01-.e+ \n\ud800'
  for (let row = 0; row < 400; row++) {
    let text = ""
    for (let index = 0; index < row % 30; index++) {
      seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0
      text += alphabet[seed % alphabet.length]
    }
    inputs.add(text)
  }
  for (const text of inputs) {
    let expected: Error | undefined
    try { parseRawSaveJson(text) } catch (error) { assert.ok(error instanceof Error); expected = error }
    if (expected === undefined) { await scanArchiveRoot(split(text, 1)); continue }
    await assert.rejects(scanArchiveRoot(split(text, 1)), error => error instanceof Error && error.name === expected!.name && error.message === expected!.message && (error as { path?: string }).path === (expected as { path?: string }).path)
  }
})


test("键目录保留非对象真实形状，不能把 scalar 或数组当空对象", { timeout: 10000 }, async () => {
  for (const [value, kind, keys] of [[null, "null", []], [false, "boolean", []], [1, "number", ["token", "decoded"]], ["xy", "string", ["0", "1"]], [[1], "array", ["0"]], [{ "2": {} }, "object", ["2"]]] as const) {
    const result = await scanArchiveRoot(split(JSON.stringify({ history_reads: value, snapshot: { accounts: value } }), 1))
    assert.deepEqual(result.keySetKinds, { history_reads: kind, "snapshot.accounts": kind })
    assert.deepEqual(result.keySets.history_reads, keys)
    assert.deepEqual(result.keySets["snapshot.accounts"], keys)
  }
})
