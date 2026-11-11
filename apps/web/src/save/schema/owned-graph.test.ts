import assert from "node:assert/strict"
import test from "node:test"
import { map, record, parseSaveField, withSaveGraphOwnership, integer, RawSaveNumber } from "./primitives.ts"
import { parseSaveJson, parseSaveSlot } from "../save-schema.ts"
import { currentSaveFixture } from "../current-save-fixture.ts"

test("私有 map 按校验顺序消费原图并保留已处理行", { timeout: 10000 }, () => {
  const first = new RawSaveNumber("1")
  const second = new RawSaveNumber("2")
  const source: Record<string, unknown> = { first, second }
  const parsed = withSaveGraphOwnership(true, () => {
    assert.equal(record(source, "map"), source, "私有 record 不应再复制容器")
    return map(source, "map", () => {}, (value, path) => {
      if (path === "map.second") assert.equal(source.first, 1, "完成一行就应释放该行 raw 值")
      return integer(value, path)
    })
  })
  assert.equal(parsed, source)
  assert.deepEqual(parsed, { first: 1, second: 2 })
  assert.notEqual(record(source, "公开"), source, "退出后公开 record 保持复制")
})

test("根字段仅在校验完成后替换且异常恢复公开隔离", { timeout: 10000 }, () => {
  const raw = { amount: new RawSaveNumber("3") }
  const root = { field: raw }
  const result = withSaveGraphOwnership(true, () => parseSaveField(root, "field", () => ({ amount: 3 })))
  assert.equal(root.field, result)
  const failure = new Error("字段拒绝")
  assert.throws(() => withSaveGraphOwnership(true, () => parseSaveField(root, "field", () => { throw failure })), error => error === failure)
  assert.equal(root.field, result)
  const publicSource = { field: raw }
  parseSaveField(publicSource, "field", () => ({ amount: 3 }))
  assert.equal(publicSource.field, raw)
  assert.notEqual(record(raw, "公开"), raw)
})

test("公开存档入口在嵌套消费作用域内仍隔离 caller 并恢复外层", { timeout: 10000 }, () => {
  const caller = currentSaveFixture()
  const before = structuredClone(caller)
  withSaveGraphOwnership(true, () => {
    assert.deepEqual(parseSaveSlot(caller), before)
    assert.deepEqual(caller, before)
    assert.throws(() => parseSaveSlot({}), /必填/)
    const source = { value: 1 }
    assert.equal(map(source, "外层", () => {}, value => value), source)
  })
  const source = { value: 1 }
  assert.notEqual(map(source, "公开", () => {}, value => value), source)
})

test("私有 map 保留原型同名字段并在失败后退出消费作用域", { timeout: 10000 }, () => {
  const source = JSON.parse('{"__proto__":1,"constructor":2,"last":3}') as Record<string, unknown>
  const failure = new Error("保留首错")
  assert.throws(() => withSaveGraphOwnership(true, () => map(source, "map", () => {}, (value, path) => {
    if (path === "map.last") throw failure
    return { value }
  })), error => error === failure)
  assert.equal(Object.getPrototypeOf(source), Object.prototype)
  assert.deepEqual(source.__proto__, { value: 1 })
  assert.deepEqual(source.constructor, { value: 2 })
  assert.equal(source.last, 3)
  assert.notEqual(record(source, "公开"), source)
})

test("消费 JSON 图保持完整对象入口的跨字段错误优先级", { timeout: 10000 }, () => {
  const cases: ((value: Record<string, any>) => void)[] = [
    value => { value.setup.config.commission_rate = "错误"; value.snapshot = {} },
    value => { value.receipt_watermark = -1; value.retail_experience = "错误" },
    value => { value.seed = false; value.information_states = "错误" },
    value => { value.information_states = { "01": {} }; value.belief_books = "错误" },
  ]
  for (const mutate of cases) {
    const input = structuredClone(currentSaveFixture()) as Record<string, any>
    mutate(input)
    const before = structuredClone(input)
    const failure = (parse: () => unknown): string => {
      try { parse() }
      catch (error) { assert.ok(error instanceof Error); return error.message }
      assert.fail("损坏存档必须拒绝")
    }
    assert.equal(failure(() => parseSaveJson(JSON.stringify(input))), failure(() => parseSaveSlot(input)))
    assert.deepEqual(input, before, "失败不得消费 caller 的对象图")
    const source = { value: 1 }
    assert.notEqual(map(source, "失败后公开", () => {}, value => value), source)
  }
})

test("公开入口不消费 caller 并保留既有 lunar_facts 引用行为", { timeout: 10000 }, () => {
  const caller = currentSaveFixture()
  const before = structuredClone(caller)
  const containers = new Map<object, string>()
  const visit = (value: unknown, inspect: (value: object, path: string) => void, path = "root"): void => {
    if (value === null || typeof value !== "object") return
    inspect(value, path)
    for (const [key, child] of Object.entries(value)) visit(child, inspect, `${path}.${key}`)
  }
  visit(caller, (value, path) => containers.set(value, path))
  const parsed = parseSaveSlot(caller)
  const shared: string[] = []
  visit(parsed, (value, path) => { if (containers.has(value)) shared.push(path) })
  assert.deepEqual(shared, [
    "root.civil_clock.policy.simulated_fallback.lunar_facts",
    "root.civil_clock.policy.simulated_fallback.lunar_facts.facts",
  ], "仅保留既有 opaque 日历引用，不能因私有消费模式新增 caller 别名")
  assert.deepEqual(caller, before)
  const expected = structuredClone(parsed)
  ;(caller as { seed: string }).seed = "999"
  assert.deepEqual(parsed, expected)
  assert.deepEqual(parseSaveJson(JSON.stringify(expected)), expected)
})
