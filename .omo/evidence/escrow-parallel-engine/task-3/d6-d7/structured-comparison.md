# D6/D7 extraction replay comparison

Result: **PASS**

Baseline: `c434f1d`

Candidate: `2b3e55b6f1343345747bb409666d0ebf3b4a619f + candidate worktree diff`

## D6 event stream

- 720 ticks; 1429 events on both sides.
- Per-tick counts, phase-tag/entity identity multisets, seq-stripped full-payload multisets, and same-entity FIFO all match.
- OrderAccepted/OrderCanceled identity facts match exactly.
- Canonical seq-stripped business multiset SHA-256: `96109015dc4a4980085996cd2a87bd9973c4a2229bbd9b27d0850ca36df97f5c`.
- 21 positions differ across 9 ticks; the JSON report records every position and both phase-tag/entity owners.

## D7 save representation

### mid

- Shared authoritative fields exact after removing the enumerated representation fields and normalizing the policy id: true.
- Common-field SHA-256: `506460fea072c5b4f18fc8b6e1da52be4ec2d92f6026d17619e7bd5fe5f7ac30`.
- 16 StrategyState values derive the 16 legacy profiles exactly: true.
- runtime_v2: poisoned=false, live_envelopes=0, next_receipt_base=0, retail_projection_seen=0.
- Explicit transformations:
  - add top-level schema_version=2
  - change setup.simulation_policy_id from a-share-simulation-v1 to a-share-simulation-v2
  - replace top-level strategy_profiles with runtime_v2.strategy_states; every legacy profile must derive exactly from the full StrategyState
  - add runtime_v2.poisoned
  - add runtime_v2.live_envelopes
  - add runtime_v2.next_receipt_base
  - add runtime_v2.retail_projection_seen

### end

- Shared authoritative fields exact after removing the enumerated representation fields and normalizing the policy id: true.
- Common-field SHA-256: `726ba581f9a164af7daebffd60af784246fc92d057f2a64dd9e94e551e382857`.
- 16 StrategyState values derive the 16 legacy profiles exactly: true.
- runtime_v2: poisoned=false, live_envelopes=0, next_receipt_base=2, retail_projection_seen=2.
- Explicit transformations:
  - add top-level schema_version=2
  - change setup.simulation_policy_id from a-share-simulation-v1 to a-share-simulation-v2
  - replace top-level strategy_profiles with runtime_v2.strategy_states; every legacy profile must derive exactly from the full StrategyState
  - add runtime_v2.poisoned
  - add runtime_v2.live_envelopes
  - add runtime_v2.next_receipt_base
  - add runtime_v2.retail_projection_seen

## Raw artifacts

Every input byte count and SHA-256 is recorded in `structured-comparison.json`.
