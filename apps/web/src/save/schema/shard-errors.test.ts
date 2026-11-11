import assert from "node:assert/strict"
import test from "node:test"
import { SchemaFailureLedger } from "./shard-errors.ts"

test("root 阶段与表内 post-check 次序优先于错误到达顺序", { timeout: 10000 }, () => {
  const ledger = new SchemaFailureLedger()
  const late = new Error("后阶段")
  const post = new Error("owner post-check")
  const leaf = new Error("较后账户的 leaf 首错")
  ledger.retain({ stage: 20, steps: [] }, late)
  ledger.retain({ stage: 10, steps: [1, { key: "1", insertion: 0 }] }, post)
  ledger.retain({ stage: 10, steps: [0, { key: "20", insertion: 1 }] }, leaf)
  ledger.retain({ stage: 30, steps: [] }, new Error("最后到达"))
  assert.equal(ledger.failure()?.error, leaf)
  assert.throws(() => ledger.throwIfAny(), error => error === leaf)
})

test("map 顺序等同 Object.keys，整数索引升序后才是字符串插入序", { timeout: 10000 }, () => {
  const keys = ["01", "4294967295", "20", "2", "0", "-0", "__proto__", "4294967294"]
  const expected = Object.keys(Object.fromEntries(keys.map(key => [key, true])))
  for (let skipped = 0; skipped < expected.length; skipped++) {
    const ledger = new SchemaFailureLedger()
    for (let insertion = keys.length - 1; insertion >= 0; insertion--) {
      const key = keys[insertion]!
      if (expected.indexOf(key) < skipped) continue
      ledger.retain({ stage: 1, steps: [{ key, insertion }] }, key)
    }
    assert.equal(ledger.failure()?.error, expected[skipped])
  }
})

test("数组索引和嵌套位置逐段比较，相同位置保留首次原错误", { timeout: 10000 }, () => {
  const ledger = new SchemaFailureLedger()
  const first = new Error("第一个")
  ledger.retain({ stage: 1, steps: [0, { key: "3", insertion: 1 }, 2] }, first)
  ledger.retain({ stage: 1, steps: [0, { key: "3", insertion: 1 }, 2] }, new Error("同位置后到"))
  ledger.retain({ stage: 1, steps: [0, { key: "3", insertion: 1 }, 9] }, new Error("后数组行"))
  assert.equal(ledger.failure()?.error, first)
  ledger.retain({ stage: 1, steps: [0, { key: "3", insertion: 1 }, 1] }, "更早数组行")
  assert.equal(ledger.failure()?.error, "更早数组行")
})

test("台账隔离 caller 的可变优先级且拒绝非法位置", { timeout: 10000 }, () => {
  const ledger = new SchemaFailureLedger()
  const entry = { key: "1", insertion: 0 }
  const position = { stage: 1, steps: [entry] }
  ledger.retain(position, "原错误")
  position.stage = 99; entry.key = "999"; position.steps.push({ key: "2", insertion: 1 })
  assert.deepEqual(ledger.failure()?.position, { stage: 1, steps: [{ key: "1", insertion: 0 }] })
  assert.equal(Object.isFrozen(ledger.failure()?.position.steps[0]), true)
  assert.throws(() => ledger.retain({ stage: -1, steps: [] }, "非法"), /stage/)
  assert.throws(() => ledger.retain({ stage: 1, steps: [NaN] }, "非法"), /steps/)
  assert.throws(() => ledger.retain({ stage: 1, steps: [{ key: "1", insertion: -1 }] }, "非法"), /insertion/)
  assert.equal(ledger.failure()?.error, "原错误")
  assert.doesNotThrow(() => new SchemaFailureLedger().throwIfAny())
})
