import { test } from "node:test"
import assert from "node:assert/strict"
import { parseNpcAttention } from "./npc-attention.ts"
import { parsePaymentFailure } from "./company/policies/shock.ts"

test("信息关注节奏严格保存，不补旧档默认值", { timeout: 10000 }, () => {
  const state = { base_probability: 0.1, next_attention_candidate_tick: "12", rng_state: "7", information_cadence: { Daily: { checks: 2 } }, next_information_check: { date: "2030-01-05", second_of_day: 43200 } }
  assert.deepEqual(parseNpcAttention(state, "npc_attention.1"), state)
  const { information_cadence: _cadence, ...withoutCadence } = state
  assert.throws(() => parseNpcAttention(withoutCadence, "npc_attention.1"))
  assert.throws(() => parseNpcAttention({ ...state, information_cadence: { Daily: { checks: 3 } } }, "npc_attention.1"))
  assert.throws(() => parseNpcAttention({ ...state, information_cadence: { EveryDays: { days: 0 } } }, "npc_attention.1"))
})

test("付款失败区分未发生支出与真实合同逾期", { timeout: 10000 }, () => {
  for (const obligation_status of ["UncommittedExpense", "ContractualPayable", "ContractualOverdue", "StatutoryPaymentFailure"] as const) {
    const state = { what: "借款偿付", amount: "1.00", obligation_status }
    assert.deepEqual(parsePaymentFailure(state, "payment_failure"), state)
  }
  assert.throws(() => parsePaymentFailure({ what: "支出", amount: "1.00" }, "payment_failure"))
})
