# Task 9 sealed corpus comparison

The comparator is tooling, not an alternative engine. It uses the accepted
ADR-0017 #6 event identity, #7 save representation, and #9 seller-fee contracts.
Prices and balances are integer cents; quantities are shares. It changes no
exchange rule or nominal fee calculation.

Run the negative gate with:

```sh
node --test --test-name-pattern=corpus_unmapped_diff scripts/simulation/escrow-corpus.test.mjs
node --test scripts/simulation/escrow-corpus.test.mjs scripts/simulation/escrow-verification-contracts.test.mjs
```

All process temporary directories must be configured under the workspace's
`.tmp/process-tmp/`; cache and logs remain under `.tmp/` as required by the Plan.

## Sealed input and provenance

`loadSealedCorpusRun(root, scenario, seed)` follows the active root index only,
checks the manifest SHA-256, sealed status and corpus digest, then checks the
selected run's SHA-256 and repeat hash. It rejects path traversal, symlinked
artifacts, duplicate run identities and unsafe JSON integers. Historical attempts
are not selected implicitly. Corpus files remain read-only and are not committed.

```sh
node scripts/simulation/escrow-corpus.mjs inspect /absolute/workspace/.omo/evidence/escrow-parallel-engine/baseline-corpus equivalence 1
```

`ADAPTED` means only that the sealed bytes were checked and projected. It is not
a semantic comparison PASS. `adaptLegacyStream(run)` returns:

- `updates`: CORPUS_SCHEMA TickFrame shapes, including quiet ticks with the exact
  empty interval `[cursor + 1, cursor]`.
- `state`: complete initial/terminal state, every tick's snapshot/order/plan/RNG
  checkpoint, save/restore checkpoints, and the external input script. Only
  `snapshot.seq` is excluded under #6; order arrival `seq` remains checked.
  The precise #7 setup policy identifier is metadata, excluded on both sides.
- Historical `checkpoint.seller_fee_projection` is a baseline-harness witness,
  not authority state. Its exact SHA-256 is retained in provenance, and the
  controlled-sell extractor validates and moves its values into typed fee
  prefixes and the seller control block before common checkpoint comparison.
  It is never silently dropped or mapped as a whole object.
- `provenance`: each original five-part identity, its original seq, and the
  derived four-part comparison key. The scope is the same phase/entity/source
  domain as `verification_evidence`; phase-6 Session variants share ordinals.
  Auxiliary scenario witnesses are identified by row and SHA-256, not claimed as
  covered by the primary stream.

Numeric event/time-series fields become decimal strings. The raw legacy
`checkpoint.timeseries_payload` container is normalized into `updates` and
snapshot state rather than compared twice under incompatible representations.
Apart from the named and hashed seller-fee harness witness above, no unknown
checkpoint or financial fields are removed. Normalizations are listed in
provenance and apply identically inside save/restore wrappers.

`prepare-current <sealed-root> <scenario> <seed> <new-request.json>` writes a
request under a canonical `ESCROW_WORKSPACE_ROOT/.tmp/` child. The
`escrow_corpus_replay` example creates a fresh v2 session, applies only the sealed
initial account allocation, runs the exact external script on the production
entry, and captures every update/conservation checkpoint and real restore. It
does not import a legacy SaveSlot. This primary replay is restricted to the
sealed no-NPC scenarios; it cannot substitute for auxiliary surface witnesses.

`diagnose-current <sealed-root> <scenario> <seed> <current-run.json>` compares
the captured full primary stream/state and validates every conservation row.
It reports `CAPTURE_MATCH` or `UNMAPPED_DIFFERENCES`, always with
`task9_acceptance: false`; differences are not automatically whitelisted. The
surface-controlled `compare` command below remains the semantic acceptance gate.

`assembleLegacyProjection(run, surfaceEvidence)` creates CORPUS_SCHEMA from the
adapted stream plus an explicitly reviewed surface extraction. Its fields are
`case_id`, `class`, `state`, `seller_fee_control`, and `corpus_control`. It adds the
complete common semantic state at `state.legacy_checkpoints`; the real current
producer must project the same semantic fields. Surface extraction is a separate
required input, not evidence created by naming a class. For example, the sealed
equivalence main run has funded initial cash: it cannot be relabelled as the
zero-cash normal-seller surface. The assembly guard rejects that false claim.
The zero-cash normal-seller and missing current-side checkpoint witnesses
remain integration requirements. The bounded extractors below expose only
sealed historical witnesses whose source fields are mechanically bound to their
orders, receipts, and checkpoint state.

The two sealed representation surfaces have a bounded mechanical extractor:

```sh
node scripts/simulation/escrow-corpus.mjs extract-controlled \
  /absolute/workspace/.omo/evidence/escrow-parallel-engine/baseline-corpus \
  representation 1 auction-rollover
node scripts/simulation/escrow-corpus.mjs extract-controlled \
  /absolute/workspace/.omo/evidence/escrow-parallel-engine/baseline-corpus \
  representation 1 cross-tick-partial-fill
```

It requires the single accepted Sell, an actual opening-auction rollover, two
Trade-bound partial fills in distinct committed ticks, a live remainder, exact
account/order/cost-basis checkpoints, and both the old fee and combined-cash
equations. It hashes the sealed script, empty strategy state, plan state,
pending intents and restore order with the same normalized semantic bytes as
the current Rust projector. `EXTRACTED_FOR_REVIEW` is not a comparison PASS;
the emitted object must be reviewed and supplied as `surface_evidence`.

The sealed equivalence run also has bounded extractors for the four buyer-side
surfaces present in its primary or directed auxiliary rows:

```sh
node scripts/simulation/escrow-corpus.mjs extract-equivalence \
  /absolute/workspace/.omo/evidence/escrow-parallel-engine/baseline-corpus \
  equivalence 1 buyer-fees
node scripts/simulation/escrow-corpus.mjs extract-equivalence \
  /absolute/workspace/.omo/evidence/escrow-parallel-engine/baseline-corpus \
  equivalence 1 t1
node scripts/simulation/escrow-corpus.mjs extract-equivalence \
  /absolute/workspace/.omo/evidence/escrow-parallel-engine/baseline-corpus \
  equivalence 1 price-cage
node scripts/simulation/escrow-corpus.mjs extract-equivalence \
  /absolute/workspace/.omo/evidence/escrow-parallel-engine/baseline-corpus \
  equivalence 1 continuous-buy-leg
```

`buyer-fees` is aggregate because public Trade facts do not carry order IDs.
`t1` records both `bought_qty` and `sold_qty`: the sealed witness is a self-trade,
so total quantity follows `after = before + bought - sold`, while `t1_locked`
increases by the Buy quantity. `continuous-buy-leg` selects the unique 100-share
terminal Buy from the sealed ID interval and Trade value. A legal immediate full
fill has no Buy `OrderAccepted`; its order identity must be proved by the
production envelope/receipt chain, not invented on the Trade.

The directed `price-cage` row retains the old accepted order ID and next-order
cursor. Its current counterpart must consume exactly one additional ID for the
P4 rejection under approved divergence #3. Exact mappings are required for the
accepted event ID, state order ID and cursor; removing these fields or calling
the surface byte-equivalent is invalid.

The two sealed divergence-9 auxiliary rows also have bounded extractors:

```sh
node scripts/simulation/escrow-corpus.mjs extract-divergence \
  /absolute/workspace/.omo/evidence/escrow-parallel-engine/baseline-corpus \
  divergence-9 1 acceptance-flip
node scripts/simulation/escrow-corpus.mjs extract-divergence \
  /absolute/workspace/.omo/evidence/escrow-parallel-engine/baseline-corpus \
  divergence-9 1 three-leg-fee-catchup
```

`acceptance-flip` keeps the zero-cash rejection frame separate from the funded
reservation control; it never splices those two historical observations into a
synthetic update. `three-leg-fee-catchup` binds each real Trade to the independent
seller's cash, position and recovered-cost changes, then checks the separately
sealed nominal-fee expectation. Both commands still print
`task9_acceptance: false`: purpose-built current production witnesses and exact
reviewed #9 mappings are required before either surface can pass.

## Exact comparison

`compareExactCorpusCase(legacy, current, mappings)` reuses `compareCorpusCase`
for class controls, #6 fact multisets and sequence coverage, acceptance evidence,
and the controlled seller terminal equation. Each mapping adds `legacy` and
`current` expected leaf values to `{divergence, effect, path}`. Paths are literal
JSON pointers in the comparator's normalized result (`updates/N/facts/N/...`),
never wildcards. A missing field is represented by `{ "absent": true }`.
Every changed leaf must match exactly one reviewed row, and every row must match
an observed difference. Whole-object replacement is rejected. #7 covers only
explicit `state/save_representation` metadata, not arbitrary saved business state.

The mapping table is an independent reviewed input; do not create it by accepting
the current diff. Approved divergence #3 is restricted to the `price-cage`
order-ID and cursor leaves above, and every current value must be exactly legacy
+ 1. Fee components, collected totals and net delivery require their
exact approved old/new values. Negative *legacy* net delivery is allowed because
the sealed pre-#9 engine debits cash on tiny fills; new delivery remains nonnegative.
Cost-chain, order/FIFO, pending inputs, strategy/plan state and RNG fields remain
equal unless an individually approved representation extraction applies.
Complete controlled checkpoints retain cash after each partial fill and around
save/restore. Those literal cash leaves may use `terminal_cash_equation` only at
the explicitly enumerated checkpoint paths; initial cash, non-cash state and
arbitrary subtrees remain outside the allowlist. Fractional configuration values
remain finite JSON fractions, while all integral evidence remains decimal text.

`corpusUnmappedDiff` first checks the positive pair and a frame-local reorder, then
requires deletion, fixed-identity payload swapping, and an unmapped payload field
to fail. The deletion reseals seq intervals, testing missing facts instead of only
a trivial range mismatch. The swap prefers two distinct payloads of the same
variant, preserving the harder identity/variant boundary.

The CLI accepts `compare request.json`, containing `sealed_root`, `scenario`,
`seed`, `surface_evidence`, the real current CORPUS_SCHEMA object `current`, and
the reviewed exact `mappings`. It prints PASS only after both the exact comparison
and negative gate pass. Stress corpus must use new-engine determinism/conservation
verification; it is rejected by the old/new comparator.
