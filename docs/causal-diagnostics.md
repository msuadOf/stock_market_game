# Offline Source Diagnostics

Run `cargo run -p engine --features simulation-diagnostics --example causal_diagnostics`.
The artifact contains a report plus immutable, sequence-addressed source facts.
All collection fields and hooks disappear when the feature is disabled. No RNG,
public event, host contract, snapshot or save representation is modified.

`price_volume_baseline` now emits `{price_volume, causal_runs}`. Each causal run
starts a real session from the input setup; it does not restore an old ledger.
The historical task-1 baseline fixture/report is unchanged.

## Definitions

- Submitted quantity is the quantity entering an allocated order-ID lifecycle,
  including settlement-aborted orders, not OrderAccepted remaining quantity.
  Validation-rejected intents never enter that lifecycle.
- Bilateral filled quantity equals twice executed market quantity. Per-order
  fill quantities and values reconcile against execution IDs, price and shares.
  Submitted equals filled + canceled + open + aborted. Filled/submitted uses
  share quantities, not order counts.
- Order market minutes use the existing 240 continuous-progress game buckets.
  Auction windows do not advance that clock. Civil seconds map opening windows
  to 09:15-09:30, continuous progress to the two sessions separated by lunch,
  and the optional closing window to 14:57-15:00. This deliberately preserves
  ADR-0011's game-bucket simplification, not 240 real continuous minutes.
- Acquisition latency uses the actual recorded acquired instant minus actual
  publication instant. The inherited decision clock omits lunch; see Task-36
  review evidence. Diagnostics do not falsify that source time to hide the defect.
- Direction persistence is the same-side fraction of adjacent actual continuous
  aggressor fills within each stock, pooled over eligible pairs. Auction trades
  have no aggressor and are excluded, explicitly.
- Impact is signed midpoint basis-point change from the pre-commit book to the
  first later valid two-sided quote. Quote sequence identifies that observation;
  intervening transitions may contribute. This is not a causal estimate.
- Depth is total displayed bid plus ask shares. A loss is any decrease between
  recorded quote snapshots, including cancellation. Recovery is the first
  market-minute observation reaching at least 50% of pre-loss depth with valid
  bilateral quotes. Recovery can legitimately be zero minutes. Unrecovered
  observations and absent impacts are null with reasons, never filled with zero.
- Decision attribution means the current execution observation at submission,
  not the original creation decision of a persistent plan. Ordinary/player
  orders have absent plan/decision references; company maps from the issuer.
- Restore emits an explicit observation restart. Reporting returns
  `RestoredObservation` because original timings cannot be recovered from a
  save; it neither invents submissions nor persists private diagnostic data.

The full offline fact vector is intentionally retained for auditability and is
not suitable for unlimited product-session collection. Large baseline consumers
must budget its memory and artifact size; product builds leave it absent.
