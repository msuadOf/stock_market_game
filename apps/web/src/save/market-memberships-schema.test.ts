import assert from "node:assert/strict"
import test from "node:test"
import { currentSaveFixture } from "./current-save-fixture.ts"
import { parseSaveSlot } from "./save-schema.ts"

// B3 适配注记：18725d07 用 u64::MAX 成员账户；main 的快照账户键域是
// JavaScript 安全整数（js_safe number 契约，上限 9007199254740991）——
// 非连续大账户断言改用 MAX_SAFE（同覆盖"非连续、远端账户"不变量）。
const maximum = "9007199254740991"
const membership = (account_id: string) => ({ account_id, admission_funding: { external_cash: "100000" } })

function sharedSave(): Record<string, unknown> {
  const save = currentSaveFixture()
  save.market_memberships = { members: { " local owner ": membership("0"), remote: membership(maximum), spaced: membership("42") } }
  save.snapshot = { ...(save.snapshot as object), accounts: Object.fromEntries(["0", "1", "42", maximum].map((id) => [id, { cash: "1", positions: {} }])) }
  save.history_reads = Object.fromEntries(["0", "1", "42", maximum].map((id) => [id, { stocks: {} }]))
  // B3 适配注记：18725d07 在此清空 corporate_actions.registries（公司域
  // 持仓勾稽，B4 面）；main fixture 无公司域，直接返回。
  return save
}

test("当前 Save 接受非连续 MAX member 与低于 AdmissionFunding 的当前现金", { timeout: 10000 }, () => {
  const save = sharedSave()
  assert.deepEqual(parseSaveSlot(save).market_memberships, save.market_memberships)
})

test("OpaqueSubjectId 接受空格及对象原型同名字符串且不丢成员", { timeout: 10000 }, () => {
  const save = sharedSave()
  save.market_memberships = { members: Object.fromEntries([["__proto__", membership("0")], [" ", membership("42")], ["constructor", membership(maximum)]]) }
  assert.deepEqual(parseSaveSlot(save).market_memberships, save.market_memberships)
})

test("MarketMembership 必填且严格拒绝未知字段与非规范身份", { timeout: 10000 }, () => {
  const missing = sharedSave()
  delete missing.market_memberships
  assert.throws(() => parseSaveSlot(missing), /market_memberships/)
  const invalid = [
    { members: {}, caps: {} },
    { members: { "": membership("0") } },
    { members: { "owner\u0085": membership("0") } },
    { members: { owner: { ...membership("0"), caps: [] } } },
    { members: { owner: { account_id: "0", admission_funding: { external_cash: "0", caps: [] } } } },
    { members: { owner: { account_id: "0" } } },
    { members: { owner: { account_id: 0, admission_funding: { external_cash: "0" } } } },
    { members: { owner: membership("01") } },
    { members: { owner: membership("18446744073709551616") } },
    { members: { owner: { account_id: "0", admission_funding: { external_cash: "-1" } } } },
  ]
  for (const market_memberships of invalid) assert.throws(() => parseSaveSlot({ ...sharedSave(), market_memberships }), /market_memberships/)
})

test("账户集合恰好为唯一 members 加 NPC，primary 0 必须 member", { timeout: 10000 }, () => {
  for (const members of [
    { owner: membership("0"), other: membership("0") },
    { owner: membership("0"), npc: membership("1") },
    { remote: membership(maximum), spaced: membership("42") },
    { owner: membership("0"), absent: membership("99") },
  ]) assert.throws(() => parseSaveSlot({ ...sharedSave(), market_memberships: { members } }), /market_memberships|账户集合/)
  for (const ids of [["0", "42", maximum], ["0", "1", "42", maximum, "99"]]) {
    const save = sharedSave()
    save.snapshot = { ...(save.snapshot as object), accounts: Object.fromEntries(ids.map((id) => [id, { cash: "1", positions: {} }])) }
    save.history_reads = Object.fromEntries(ids.map((id) => [id, { stocks: {} }]))
    assert.throws(() => parseSaveSlot(save), /账户集合/)
  }
})
