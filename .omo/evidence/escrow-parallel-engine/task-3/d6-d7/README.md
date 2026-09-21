# D6/D7 raw replay witness

This directory preserves the two actual `extraction_replay` captures used to re-pin the
escrow-v2 anchors.  It is evidence, not a second implementation or a comparator that can
silently broaden an allowed divergence.

## Inputs

- Baseline: `c434f1d`, with `a-share-simulation-v1` save representation.
- Candidate: `2b3e55b` plus the candidate engine-source overlay.  The capture predates the
  later test-only B2 regression addition; that addition cannot affect the engine output.
- Scenario: `packages/engine/tests/extraction_replay.rs`, seed `0x5EED_2026_0903`, two SSE
  main-board stocks, three 240-tick trading days.

The eight `*-events*.json` / `*-save-*.json` files are unmodified serialized outputs.  SHA-256 of
the raw event streams is baseline `ab0001e0da70e33424ccd406eead810055ae7afb21324230c56dff16fea9afc8`
and candidate `e8e5661644a24e0341aa3e1c980251beb52759a131da1528bd82ddf72d99bece`; each has 1,429
events.  The differing raw hashes are expected because D6 permits stable in-tick presentation
ordering and D7 changes the save representation.

## D6 check

`*-events-normalized.json` removes only display `seq`, retains the originating tick and every
business payload field, sorts by the retained complete event payload, and is byte-identical on
both sides:

```
550dc6bd1fdf6198af7bfbb436db04d03bc49f46c575b92e0f9c219e46e650b7
```

The capture comparison found 21 changed raw positions, all within their original tick; no event
was deleted, moved across a tick, or changed in business identity/payload.  Same-entity FIFO and
order identity therefore remain in the complete retained payload rather than being ignored by a
global multiset-only check.

## D7 check

The `*-save-*-common.json` files retain every save field except the approved representation
boundary: `schema_version`, `runtime_v2`, `setup.simulation_policy_id`, and the legacy profile
versus full `StrategyState` transport projection.  The paired common files are byte-identical:

| witness | SHA-256 |
| --- | --- |
| mid-save common | `615ea983643d7a97c21b6425194b88610b6e1c98b22ca403318c0d5a6316acd9` |
| end-save common | `dfedaeaae2ed7bc0fcbb805ef7ac27d0bd34ee300f464e1dc244c3ea72ec7197` |
| mid-save profiles derived from `StrategyState` | `d3c499750fa22e95570f2aa0864bbb18278e3444e34475ae682aea8dc87b27d7` |

An auditor can verify every listed equality with `sha256sum` and `diff -q` on the paired files.
Any mismatch, including a non-approved save field, is evidence failure rather than an implicit
fallback.

## Structured comparison

`structured-comparison.json` is the persisted, machine-readable comparison produced from the
same eight raw captures. It records every changed in-tick position, all D6 equality predicates,
the D7 representation transformations, and the input byte hashes. `structured-comparison.md`
is its human-readable summary. The comparison is evidence only; it is not executable verifier
logic.
