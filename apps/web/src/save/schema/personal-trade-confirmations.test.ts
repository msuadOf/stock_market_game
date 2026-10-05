import assert from "node:assert/strict"
import test from "node:test"
import { parseSaveRuntime } from "./runtime-state.ts"

const confirmation = { receipt_id: "0", civil_date: "2030-01-02", code: "600000", side: "Buy", price: "1000", quantity_shares: 100, gross: "100000", actual_fees: { commission: "500", stamp_tax: "0", transfer_fee: "1" } }
const runtime = { poisoned: false, next_receipt_base: "2", live_envelopes: [], retail_projection_seen: [], strategy_states: {}, personal_trade_confirmations: { "0": [confirmation] } }

test("存档本人交割单为必填完整事实并严格校验receipt与分股因果", { timeout: 10000 }, () => {
  assert.deepEqual(parseSaveRuntime(runtime).personal_trade_confirmations, runtime.personal_trade_confirmations)
  const { personal_trade_confirmations: removed, ...missing } = runtime
  assert.ok(removed)
  assert.throws(() => parseSaveRuntime(missing), /personal_trade_confirmations/)
  for (const changed of [
    { ...confirmation, receipt_id: "2" },
    { ...confirmation, receipt_id: "00" },
    { ...confirmation, gross: "99999" },
    { ...confirmation, quantity_shares: 4294967296 },
    { ...confirmation, actual_fees: { ...confirmation.actual_fees, commission: "-1" } },
  ]) assert.throws(() => parseSaveRuntime({ ...runtime, personal_trade_confirmations: { "0": [changed] } }), /personal_trade_confirmations/)
  assert.throws(() => parseSaveRuntime({ ...runtime, personal_trade_confirmations: { "0": [confirmation, confirmation] } }), /receipt/)
  assert.throws(() => parseSaveRuntime({ ...runtime, personal_trade_confirmations: { "0": [confirmation], "1": [confirmation] } }), /receipt/)
})
