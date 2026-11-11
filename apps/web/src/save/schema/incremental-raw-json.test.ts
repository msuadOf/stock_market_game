import assert from "node:assert/strict"
import test from "node:test"
import { IncrementalRawSaveParser } from "./incremental-raw-json.ts"
import { parseRawSaveJson } from "./raw-json.ts"

function capture(operation: () => unknown): unknown {
  try { return { value: operation() } }
  catch (error) {
    assert.ok(error instanceof Error)
    return { name: error.name, message: error.message, path: (error as { path?: string }).path }
  }
}
function chunks(text: string, widths: readonly number[]): unknown {
  const parser = new IncrementalRawSaveParser()
  for (let cursor = 0, index = 0; cursor < text.length; index++) {
    const width = widths[index % widths.length]!
    parser.feed(text.slice(cursor, cursor + width))
    cursor += width
  }
  return parser.finish()
}

const inputs = [
  ' {"中文😀":"值𠮷","array":[0,-2,1.25,1e+2,true,false,null,{},[]],"escaped":"a\\\"b\\n\\u4e2d"} ',
  '{"__proto__":{"safe":true},"constructor":[],"unicode":"\\ud83d\\ude00"}',
  '"孤立\ud800"', String.raw`"\ud800"`, String.raw`{"\ud800":1}`,
  '0', '-0', '0.0', '0e0', '9007199254740993', '1e309', '', ' ',
  'true', 'false', 'null', 'nullx', '[nullx]', '{"x":truex}',
  'tru', 'falsex', 'nul}', 'nulf,', 'foo', '+1', '01', '1.', '1e',
  '{', '[', '[1', '{"x"', '{"x":', '{"x":1', '{"x" 1}', '[1 2]',
  '[1,]', '{"a":1,}', '"unfinished', '"escape\\', '"bad\\q"', '"line\nfeed"',
  '{} []', '{"a":1,"a":2}', String.raw`{"a":1,"\u0061":2}`,
  '{"outer":[{"a":1,"a":2}]}', '[{"x":}]', '{"a":[,]}',
]

test("增量 parser 对全部单一切分点保持旧词法结果和首错", { timeout: 10000 }, () => {
  for (const text of inputs) {
    const expected = capture(() => parseRawSaveJson(text))
    for (let split = 0; split <= text.length; split++) {
      assert.deepEqual(capture(() => {
        const parser = new IncrementalRawSaveParser()
        parser.feed(text.slice(0, split))
        parser.feed(text.slice(split))
        return parser.finish()
      }), expected, `切分 ${split}，输入 ${JSON.stringify(text)}`)
    }
  }
})

test("逐字符及确定性混合块保持嵌套、转义和 number token", { timeout: 10000 }, () => {
  for (const text of inputs) {
    for (const widths of [[1], [2, 7, 1, 13], [11, 3, 5]]) {
      assert.deepEqual(capture(() => chunks(text, widths)), capture(() => parseRawSaveJson(text)))
    }
  }
})

test("完整重复键和结构错误在 feed 当场拒绝，不汇集全文", { timeout: 10000 }, () => {
  const duplicate = new IncrementalRawSaveParser()
  duplicate.feed('{"x":1,')
  assert.throws(() => duplicate.feed('"x"'), /重复 JSON 键/)
  const colon = new IncrementalRawSaveParser()
  colon.feed('{"x"')
  assert.throws(() => colon.feed('!'), /JSON 字段缺少冒号/)
})

test("所有 EOF 前缀及确定性损坏语料保持旧错误顺序", { timeout: 10000 }, () => {
  const valid = '{"x":[true,false,null,-2.5e+3,"a\\n\\u4e2d",{"nested":[]}],"y":{}}'
  const candidates = new Set<string>()
  for (let index = 0; index <= valid.length; index++) {
    candidates.add(valid.slice(0, index))
    candidates.add(valid.slice(0, index) + valid.slice(index + 1))
    candidates.add(valid.slice(0, index) + "!" + valid.slice(index))
  }
  let seed = 42
  const alphabet = '{}[]:,"\\nulltruefalse01-.e+ \n\ud800'
  for (let row = 0; row < 400; row++) {
    let text = ""
    for (let index = 0; index < row % 25; index++) {
      seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0
      text += alphabet[seed % alphabet.length]
    }
    candidates.add(text)
  }
  for (const text of candidates) {
    assert.deepEqual(capture(() => chunks(text, [1, 3, 7])), capture(() => parseRawSaveJson(text)), JSON.stringify(text))
  }
})

test("多个大 chunk 的长字符串与非空数组不丢失内容，终态禁止继续解析", { timeout: 10000 }, () => {
  const text = JSON.stringify({ long: "中文😀\\转义\n".repeat(12000), rows: Array.from({ length: 300 }, (_, index) => ({ id: index, value: `row-${index}` })) })
  assert.deepEqual(chunks(text, [65536, 3, 16384]), parseRawSaveJson(text))
  const complete = new IncrementalRawSaveParser()
  complete.feed("{}")
  assert.deepEqual(complete.finish(), {})
  assert.throws(() => complete.feed(" "), /已结束/)
  assert.throws(() => complete.finish(), /已结束/)
  const disposed = new IncrementalRawSaveParser()
  disposed.feed('{"pending":[')
  disposed.dispose()
  assert.throws(() => disposed.finish(), /已结束/)
})

test("继承 setter 不参与 raw 值写入且词法失败保持终态", { timeout: 10000 }, () => {
  const key = "incrementalSetterProbe"
  let writes = 0
  const descriptor = Object.assign(Object.create(null) as PropertyDescriptor, {
    set() { writes++ }, configurable: true,
  })
  Object.defineProperty(Object.prototype, key, descriptor)
  try {
    const parsed = chunks(`{"${key}":{"nested":true}}`, [1]) as Record<string, unknown>
    assert.ok(Object.hasOwn(parsed, key))
    assert.deepEqual(parsed[key], { nested: true })
    assert.equal(writes, 0)
  } finally { delete (Object.prototype as Record<string, unknown>)[key] }
  const failed = new IncrementalRawSaveParser()
  assert.throws(() => failed.feed("}"), /包含非法 JSON 数值或值/)
  assert.throws(() => failed.feed("{}"), /已结束/)
})
