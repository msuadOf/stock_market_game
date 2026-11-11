import assert from "node:assert/strict"
import test from "node:test"
import { scanArchiveRoot, type ArchiveRecord, type ArchiveContainerShape } from "./archive-root-scan.ts"
import { RawSaveNumber } from "./primitives.ts"
import { parseInformationStates } from "./personal/information.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"

async function* chunks(text: string): AsyncIterable<string> {
  for (let offset = 0; offset < text.length; offset += 7) yield text.slice(offset, offset + 7)
}

test("逐账户和逐 receipt 输出真实 raw 记录，不汇集整表", { timeout: 10000 }, async () => {
  const records: ArchiveRecord[] = []
  const shapes: ArchiveContainerShape[] = []
  const result = await scanArchiveRoot(chunks('{"information_states":{"20":{"owner":"20","companies":{}},"2":{"owner":"2","companies":{}}},"personal_trade_confirmations":{"2":[{"n":1},{"n":2}],"3":[]}}'), {
    records: [["information_states", "*"], ["personal_trade_confirmations", "*", "*"]],
    onRecord: record => { records.push(record) }, onShape: shape => { shapes.push(shape) },
  })
  assert.deepEqual(records.map(record => record.path), [["information_states", "20"], ["information_states", "2"], ["personal_trade_confirmations", "2", 0], ["personal_trade_confirmations", "2", 1]])
  assert.deepEqual(records.map(record => record.insertion), [0, 1, 0, 1])
  assert.deepEqual(records[2]?.value, { n: new RawSaveNumber("1") })
  assert.deepEqual(shapes.find(shape => shape.path.join(".") === "information_states")?.keys, ["2", "20"])
  assert.equal(shapes.find(shape => shape.path.join(".") === "personal_trade_confirmations.3")?.length, 0)
  assert.equal(Object.hasOwn(result.rawMetadata, "information_states"), false)
  assert.equal(result.schemaValidated, false)
})

test("真实 INFO leaf 错误按枚举键排序，后发现词法错仍优先", { timeout: 10000 }, async () => {
  const ledger = new SchemaFailureLedger()
  const source = '{"information_states":{"20":{"owner":"bad","companies":{}},"2":{"owner":"wrong","companies":{}}}}'
  await scanArchiveRoot(chunks(source), { records: [["information_states", "*"]], onRecord: record => {
    const key = String(record.path[1])
    try { parseInformationStates({ [key]: record.value }) }
    catch (error) { ledger.retain({ stage: 1, steps: [{ key, insertion: record.insertion }] }, error) }
  } })
  assert.throws(() => ledger.throwIfAny(), /information_states.2.owner/)
  await assert.rejects(scanArchiveRoot(chunks(source.slice(0, -1) + ',"x":1,"x":2}'), {
    records: [["information_states", "*"]], onRecord: () => { throw new Error("callback 首错") },
  }), /重复 JSON 键/)
})

test("非预期 sink 错误不冒充词法错误，流错误仍优先", { timeout: 10000 }, async () => {
  const failure = new Error("sink 失败")
  const options = { records: [["information_states", "*"]], onRecord: () => { throw failure } }
  await assert.rejects(scanArchiveRoot(chunks('{"information_states":{"2":{}}}'), options), error => error === failure)
  const stream = async function* (): AsyncIterable<string> {
    yield '{"information_states":{"2":{}}}'
    throw new Error("gzip 后错")
  }
  await assert.rejects(scanArchiveRoot(stream(), options), /gzip 后错/)
})

test("记录表为 scalar 时仍交付真实形状，不能漏过 object 门禁", { timeout: 10000 }, async () => {
  const shapes: ArchiveContainerShape[] = []
  await scanArchiveRoot(chunks('{"information_states":null,"history_reads":1,"price_memories":"bad"}'), {
    records: [["information_states", "*"], ["history_reads", "*"], ["price_memories", "*"]],
    onRecord: () => assert.fail("scalar 没有账户行"), onShape: shape => { shapes.push(shape) },
  })
  assert.deepEqual(shapes.filter(shape => shape.path.length === 1).map(shape => [shape.path[0], shape.kind]), [["information_states", "null"], ["history_reads", "number"], ["price_memories", "string"]])
})

test("记录路径拒绝空路径、通配前缀相交及与保留元数据重叠", { timeout: 10000 }, async () => {
  for (const records of [[[]], [["table"], ["table", "*"]], [["table", "*"], ["table", "2", "*"]]]) {
    await assert.rejects(scanArchiveRoot(chunks("{}"), { records, onRecord: () => {} }), /路径.*空|路径.*重叠/)
  }
  await assert.rejects(scanArchiveRoot(chunks("{}"), { records: [["setup", "*"]], onRecord: () => {} }), /元数据.*重叠/)
})
