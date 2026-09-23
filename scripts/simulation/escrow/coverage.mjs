import assert from "node:assert/strict";
import { feasibility } from "./compare.mjs";

export function inspectRun(records, scenario) {
  const frames = records.filter((record) => record.kind === "TickFrame");
  const witnesses = records.filter((record) => record.kind === "divergence_witness");
  const ids = new Set([6]);
  const facts = frames.flatMap((frame) => frame.events.map((fact) => fact.event));
  assert(facts.some((event) => event.AuctionTick), "auction surface absent");
  assert(facts.some((event) => event.DayBoundary), "day boundary absent");
  for (const witness of witnesses) {
    for (const frame of [witness.frame, witness.control, witness.treatment].filter(Boolean)) feasibility([frame]);
    switch (witness.name) {
      case "cage-reject-then-valid-control":
        assert(witness.frame.events.some(({ event }) => event.IntentRejected?.reason === "PriceCageExceeded"));
        assert(witness.frame.events.some(({ event }) => event.OrderAccepted));
        ids.add(3); ids.add(8); break;
      case "sealed-cancel-reuses-cash-old-side":
        assert(witness.frame.events.some(({ event }) => event.OrderCanceled));
        assert(witness.frame.events.some(({ event }) => event.OrderAccepted));
        ids.add(2); break;
      case "same-tick-place-cancel":
        assert(witness.frame.events.some(({ event }) => event.OrderAccepted));
        assert(witness.frame.events.some(({ event }) => event.OrderCanceled));
        ids.add(4); break;
      case "auction-zero-quantity-prevalidation":
        assert(witness.frame.events.some(({ event }) => event.SettlementError));
        ids.add(5); break;
      case "player-takes-real-institution-child-before-chain":
        assert(witness.treatment.events.some(({ event }) => event.Trade?.taker === 0));
        assert.notDeepEqual(witness.control_plans, witness.treatment_plans, "empty chain witness");
        ids.add(1); break;
      default: throw new Error(`unclassified witness: ${witness.name}`);
    }
  }
  if (scenario === "equivalence") {
    const trading = frames.find((frame) => frame.events.filter(({ event }) => event.Trade).length >= 2);
    assert(trading, "normal seller multi-leg terminal missing");
    assert.equal(trading.snapshot.accounts[0].reserved_cash, 0);
    const controls = records.filter((record) => record.kind === "share_control");
    assert.equal(controls.length, 2);
    assert(controls.every((record) => record.frame.events.some(({ event }) => event.IntentRejected?.reason === "InsufficientShares")));
  }
  if (scenario === "divergence-9") {
    const boundary = records.find((record) => record.kind === "acceptance_boundary");
    assert.equal(boundary.old_funded.snapshot.accounts[0].reserved_cash, 400);
    assert(boundary.old_zero_cash.events.some(({ event }) => event.IntentRejected?.reason === "InsufficientCash"));
    const independent = records.find((record) => record.kind === "independent_seller_fee_control");
    assert.deepEqual(independent.legs.map((leg) => leg.observed_net_cents), [-400, 100, 999]);
    assert.deepEqual(independent.legs.map((leg) => leg.observed_fee_cents), [500, 0, 1]);
    ids.add(9);
  }
  if (scenario === "representation") {
    const before = records.find((record) => record.kind === "before_save");
    const after = records.find((record) => record.kind === "after_restore");
    assert.deepEqual(before.state, after.state);
    assert(Object.values(before.state.resting_orders).flat().some((order) => order.side === "Sell" && order.filled_qty > 0 && order.qty > 0));
    ids.add(7);
  }
  if (scenario === "stress") {
    const terminal = records.find((record) => record.kind === "terminal");
    assert.equal(terminal.baseline_invariants.checked_ticks, frames.length);
    for (const field of ["shares_conserved", "nonnegative_cash", "t1_bounded", "reservations_bounded", "order_quantities_consistent"]) assert.equal(terminal.baseline_invariants[field], true);
  }
  return { observed_divergence_surfaces: [...ids].sort(), witness_names: witnesses.map((record) => record.name), comparison: feasibility(frames) };
}
