import assert from "node:assert/strict"
import test from "node:test"
import { validateMembershipAccounts } from "./market-memberships.ts"

type Setup = Parameters<typeof validateMembershipAccounts>[1]
type Snapshot = Parameters<typeof validateMembershipAccounts>[2]
const memberships = { members: { owner: { account_id: "0", admission_funding: { external_cash: "0" } }, remote: { account_id: "42", admission_funding: { external_cash: "0" } } } }
const setup = (enabled: boolean, company = true) => ({ npcs: { retail_count: 1, inst_count: 1, hot_count: 0 }, issuer_repurchase_enabled: enabled, ...(company ? { company_system: { mode: "Simple" } } : {}) }) as unknown as Setup
const snapshot = (...ids: string[]) => ({ accounts: Object.fromEntries(ids.map(id => [id, { cash: "0", positions: {} }])) }) as unknown as Snapshot

test("公司局启用回购时必须包含 NPC 后的确定性专户", { timeout: 10000 }, () => {
  assert.doesNotThrow(() => validateMembershipAccounts(memberships, setup(true), snapshot("0", "1", "2", "3", "42")))
  for (const ids of [["0", "1", "2", "42"], ["0", "1", "2", "4", "42"], ["0", "1", "2", "3", "4", "42"]]) {
    assert.throws(() => validateMembershipAccounts(memberships, setup(true), snapshot(...ids)), /账户集合/)
  }
})

test("禁用回购或没有公司系统时不增加专户", { timeout: 10000 }, () => {
  for (const state of [setup(false), setup(true, false)]) {
    assert.doesNotThrow(() => validateMembershipAccounts(memberships, state, snapshot("0", "1", "2", "42")))
    assert.throws(() => validateMembershipAccounts(memberships, state, snapshot("0", "1", "2", "3", "42")), /账户集合/)
  }
})

test("专户与成员集合按 Rust 并集去重，NPC 身份仍不可作为成员", { timeout: 10000 }, () => {
  const shared = { members: { owner: memberships.members.owner, treasury: { account_id: "3", admission_funding: { external_cash: "0" } } } }
  assert.doesNotThrow(() => validateMembershipAccounts(shared, setup(true), snapshot("0", "1", "2", "3")))
  const npc = { members: { owner: memberships.members.owner, invalid: { account_id: "2", admission_funding: { external_cash: "0" } } } }
  assert.throws(() => validateMembershipAccounts(npc, setup(true), snapshot("0", "1", "2", "3")), /成员账户/)
})

test("没有 NPC 时回购专户的确定性账户为 1", { timeout: 10000 }, () => {
  const zero = { ...setup(true), npcs: { retail_count: 0, inst_count: 0, hot_count: 0 } } as Setup
  assert.doesNotThrow(() => validateMembershipAccounts(memberships, zero, snapshot("0", "1", "42")))
  assert.throws(() => validateMembershipAccounts(memberships, zero, snapshot("0", "2", "42")), /账户集合/)
})
