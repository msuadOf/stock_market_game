export const surfaces = [
  { class: "i", scenario: "equivalence", construction: "No NPC/plan feedback. Owned sellable shares; 1000-cent sell 300 shares, same-tick 100+200-share buy legs; zero old reservation, nominal fees covered at every prefix.", surfaces: ["normal multi-leg seller terminal", "cumulative minimum commission", "T+1", "order identity", "continuous matching", "snapshot visibility", "auction and day boundary"] },
  { class: "ii", scenario: "divergence-9", construction: "Single sell 1200 shares at one cent; exogenous buys 100,100,1000 shares yield gross 100,100,1000 cents. Separate 100-share one-yuan zero-cash/funded acceptance controls with owned sellable inventory.", surfaces: ["positive old seller reservation", "acceptance flip", "prefix fee shortfall", "later recovery", "nominal versus collected fees"] },
  { class: "iii", scenario: "stress", construction: "180 ticks, three trading days, five SH/SZ stocks and 26 active NPCs (12 retail, 10 institution, 4 hot). New-engine-only determinism/conservation/safety target, not historical fidelity. Separate paired player/real institution-child witness when present.", surfaces: ["bounded repeated execution", "decision chain", "auction", "day boundary", "repeated fees", "T+1 unlock"] },
  { class: "iv", scenario: "representation", construction: "Cash-rich single live low-price sell rolls from opening auction; partial fill; save before continuation; immediate restore equality; explicit identical exogenous continuation; no strategies/plans. RNG cursor emitted each tick.", surfaces: ["auction rollover live sell", "cross-tick partial fill", "before-save projection", "after-restore projection", "every continuation tick", "order arrival/FIFO", "RNG", "cost chain"] },
];

export const transformations = [
  { id: 1, paths: ["plans", "pending_plan_events"], rule: "DecisionSnapshot visibility only; controlled corpus disables feedback; no blanket output allowance" },
  { id: 2, paths: ["snapshot.accounts.*.reserved_cash", "snapshot.accounts.*.reserved_sell_qty"], rule: "P0 before allocation; sealed releases next allocation; committed snapshots immediately visible" },
  { id: 3, paths: ["next_order_id", "events.*.event.OrderAccepted.id"], rule: "Only preallocated ID consumed by stock-stage refusal" },
  { id: 4, paths: ["events.*.event.OrderCanceled"], rule: "Only cross-envelope same-tick cancellation removal" },
  { id: 5, paths: ["events.*.event.SettlementError", "events.*.event.IntentRejected"], rule: "Only prepaid-account constraint migration" },
  { id: 6, paths: ["events.*.event.*.seq", "events"], rule: "Frame-local permutation/seq scalar only; identity/payload multiplicity strict; tick order, seq ranges and timeseries strict" },
  { id: 7, paths: ["state.setup.simulation_policy_id", "state"], rule: "Explicit v2 representation only; restore behavior, RNG, orders and cost chain preserved" },
  { id: 8, paths: ["events.*.event.IntentRejected", "next_order_id"], rule: "Cage stock-stage validation position only; no changed cage formula" },
  { id: 9, paths: ["snapshot.accounts.0.reserved_cash", "seller_fee_projection", "acceptance_boundary"], rule: "Sell reserve becomes zero; old cash shortfall rejection becomes acceptance; charged_delta=min(F_after-charged_before,gross); priority commission/stamp/transfer; terminal cash delta=F_final-charged_final; invested/recovered gross chain unchanged" },
];
