import assert from "node:assert/strict"
import test from "node:test"
import { parseSaveRuntimeV2 } from "./schema/runtime-v2.ts"

const ONE = "3ff0000000000000"
const SMALL = "3f847ae147ae147b"

function strategyStates(positionStep: unknown): Record<string, Record<string, Record<string, unknown>>> {
  return {
    "1": { ZiNoise: {
      retail_style: "Noise", arrival_rate: ONE, order_size_mean: 100,
      chase_prob: SMALL, tick_cents: 1, dip_threshold: SMALL,
      stop_loss_threshold: SMALL, take_profit_threshold: SMALL,
      volume_confirmation: ONE, position_step_bp: positionStep,
      base_observation_probability: ONE,
    } },
    "2": { BeliefInstitution: {
      style: "DeepValue", margin: SMALL, order_size: 100,
      position_step_bp: positionStep, base_observation_probability: ONE,
    } },
    "3": { Momentum: {
      style: "Momentum", lookback: 2, trend_threshold: SMALL,
      order_size: 100, volume_confirmation: ONE, base_observation_probability: ONE,
    } },
  }
}

function runtime(strategyStates: unknown) {
  return {
    poisoned: false, next_receipt_base: "0", live_envelopes: [],
    retail_projection_seen: [], strategy_states: strategyStates,
  }
}

test("runtime strategies preserve position adjustment steps and uncapped momentum state", () => {
  for (const step of [0, 750, 10_000]) {
    const input = runtime(strategyStates(step))
    assert.deepEqual(parseSaveRuntimeV2(input), input)
  }
})

test("runtime position adjustment steps must be explicit integers from zero to ten thousand", () => {
  for (const [account, variant] of [["1", "ZiNoise"], ["2", "BeliefInstitution"]] as const) {
    const expected = new RegExp(`strategy_states\\.${account}\\.${variant}\\.position_step_bp`)
    const missing = strategyStates(750)[account][variant]
    delete missing.position_step_bp
    assert.throws(() => parseSaveRuntimeV2(runtime({ [account]: { [variant]: missing } })), expected)
    for (const invalid of [-1, 10_001, 0.5, "750", null, Number.NaN, Number.POSITIVE_INFINITY]) {
      const payload = strategyStates(invalid)[account][variant]
      assert.throws(() => parseSaveRuntimeV2(runtime({ [account]: { [variant]: payload } })), expected)
    }
  }
})

test("runtime strategies reject the removed stock fraction cap without migrating it", () => {
  for (const [account, variant] of [["1", "ZiNoise"], ["2", "BeliefInstitution"], ["3", "Momentum"]] as const) {
    const payload = { ...strategyStates(750)[account][variant], max_stock_fraction: ONE }
    assert.throws(
      () => parseSaveRuntimeV2(runtime({ [account]: { [variant]: payload } })),
      new RegExp(`strategy_states\\.${account}\\.${variant}\\.max_stock_fraction`),
    )
  }
})
