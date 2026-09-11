VERDICT: APPROVE

# Task 24 Independent Review — 复用母单和工作单实现统一计划执行 (commit ccf0490)

Reviewer: independent subagent (did not implement). Branch `codex/feat/web-ui-polish`.
Scope: `git show ccf0490` — 16 files, 1558+/2−. Review-only; no product files touched.

## Verification runs (all performed by reviewer in current tree)

| Check | Command | Result |
|---|---|---|
| Focused suite | `cargo test -p engine --test plan_execution` | 12 passed / 0 failed (gold 3 + failures 9) — matches worker claim |
| Full suite | `cargo test -p engine` (summed all suites) | 39 suites, **939 passed / 0 failed / 4 ignored** — matches worker claim |
| Scoped clippy | `cargo clippy -p engine --lib -- -D warnings -A clippy::unnecessary_filter_map` | exit 0 |
| Scoped clippy | `cargo clippy -p engine --test plan_execution -- …` | exit 0 |
| All-targets clippy | `cargo clippy -p engine --all-targets -- -D warnings -A clippy::unnecessary_filter_map` | exit 101, red ONLY in `tests/analysis_profiles/invariants.rs` (3× unusual digit groupings) + `tests/experience_feedback/main.rs` (2× too_many_arguments, 1× bool_assert_comparison) — exactly the pre-existing task-17/20 debt; **zero task-24 files** |
| Dirty worktree | `git diff HEAD --stat -- packages/engine` | empty — all task-24 product files clean post-commit |

## Findings

1. **[non-blocking] A-share cancel-window semantics faithfully mirrored.**
   `plan_child_is_cancellable_now` (actions.rs:185-193) returns: Continuous → true; CallAuction → `tick % ticks_per_day < auction_ticks / 3`; PreOpen/ClosingAuction → false. This is a verbatim mirror of the existing authoritative rules at `self_views.rs:55`, `execution/reconcile.rs:127`, and `auction.rs:100/157` (`cancelable_ticks = auction_ticks / 3` = the game's established 09:20 opening-auction boundary; closing auction non-cancellable, matching real 沪深 rules). No new market rule invented.

2. **[non-blocking] Single order/freeze/settle path confirmed — no second system.**
   Every submit/cancel flows through `route_plan_intent` → existing `route_intent` / `route_auction_intent` (routing.rs:80-105). T+1 sell availability and board-lot rules are enforced by the existing router prevalidation (session.rs:2273-2344: `sellable_qty` → InsufficientShares; Buy `qty.is_multiple_of(lot_size)`; Sell exact-odd-remainder `qty % lot_size == available_sell % lot_size`), which the executor cannot bypass. Reservation pre-checks reuse the router's own `buy_order_reservation` / `sell_order_fee_reservation` (actions.rs:24-31). The sub-lot buy guard at plan_execution.rs:93 mirrors the existing NPC guard at orders.rs:211 exactly (no overbuy, no fabricated illegal order); `child.qty > child.remaining` rejection prevents over-target submission. `linked_plan_id` is `#[serde(default, skip_serializing_if = "Option::is_none")]` (execution.rs:28-30) — legacy saves byte-identical.

3. **[non-blocking] Queue-priority honesty locked by real behavior assertions.**
   Adoption only on exact (side, price, qty) match keeps the original `OrderId` with zero events (failures/adoption.rs:33-58 asserts `order_id == old_id` AND `report.events.is_empty()` — no cancel/resubmit churn that would silently lose priority). Changed price must really cancel then really submit: failures/adoption.rs:105-155 asserts `Replaced { canceled_order_id == old_id }`, `new_id != old_id`, and the exact event slice `[OrderCanceled, OrderAccepted]` with matching ids. Day-end revival test asserts the next-day reissue gets a fresh id (gold.rs:127-130).

4. **[non-blocking] Reachability lesson (task-21) applied — no dead success paths found in delivered surface.**
   Reviewed every guard for overlap with success conditions: the day-end producer filters `filled_qty < target_qty` on the PARENT (synchronization.rs:65), so a closing fill that completes the plan cannot also emit `TradingDayEnded` (gold.rs:134-188 locks Completed-not-Terminated). `Accepted` is always pushed before subsequent `Filled` pending events: same-tick crossing (submit that immediately trades against a resting counterparty) is proven ordered by gold.rs:7-58 (fill of 100 applied after acceptance, `filled_qty=100`, status Active); auction acceptance ids are proven consistent (`OrderId(next_order_id)` matches book ids in auctions tests). Auction→continuous remainder keeps its link and id (auctions.rs:49-93, asserts carried `resting_orders[0].id == active_id`).

5. **[non-blocking] Task-23-style quantity boundary probes all present and real.**
   Failed cancel keeps old child + reservation (cancellation.rs:7-50: `RouteRejected{OrderNotFound}`, resting order unchanged). Allocation shortfall errors BEFORE any order exists (cancellation.rs:53-79: `AllocationInsufficient{allocated_cents: 0}`, empty book). Odd-lot: 350/400 fill leaves remainder 50 → `RemainingBelowBoardLot{50}`, no new order, `filled_qty` stays 350 (remainder.rs:6-74) — remainder neither rounded up into overbuy nor oversold. Second in-flight child per account+stock → `IncompatibleExecutionState` with book unchanged (adoption.rs:61-102). Assertions lock state, ids, and event shapes — not trivially-true.

6. **[non-blocking] Quality gates met.**
   No `unwrap()/expect()/panic!/let _ =/.ok()` anywhere in `session/plan_execution*` (grep verified) — 铁律二 respected; all failures are typed `PlanExecutionError` variants or observable `RouteRejected`/`SettlementFailed` dispositions derived from real router events. The `QuoteAction` match is exhaustive with no catch-all (frozen task-23 API). New file sizes (total lines, before subtracting comments): plan_execution.rs 157, actions.rs 205, routing.rs 154, synchronization.rs 74, types.rs 144 — all under the 250 ceiling. `PendingPlanEvent` derives no serde (compile-time guarantee it cannot enter saves); GameSession save remains manual and SaveSlot is untouched by this commit. Transactional sync clones the PlanBook and swaps only after all events apply (synchronization.rs:12-47) — on error, pending events are retained and the book unchanged.

7. **[non-blocking] Scope discipline verified.**
   `git show ccf0490 --stat` = exactly the 16 claimed files; every hunk is required by the task (module wiring, `linked_plan_id` plumbing incl. the necessary `&& plan.linked_plan_id.is_none()` completion-ownership guard in records.rs:53, day-end hook, `linked_plan_id: None` backfill in orders.rs and 3 existing test fixtures, new adapter + tests). No unrelated changes smuggled in; no existing assertions weakened (session.rs test diffs are pure field additions). The 4 tracked `apps/web/src/types/generated/*.ts` currently dirty in the worktree are ts_rs regeneration churn from test runs (ParentOrderPlan.ts adds only the optional `linked_plan_id?: PlanId`) — generated dir is owned by task 29 per repo convention; `packages/engine` itself is fully clean.

8. **[non-blocking] Wiring-contract notes for tasks 26/27 (not defects in delivered scope).**
   (a) `record_plan_execution_day_end` emits `TradingDayEnded` only for plans that still had a linked parent (filled<target) that day; a plan that never submits receives no day-end event, so horizon expiry advances only on days it had a child. Wholesale plan day-end driving belongs to task 26/27 per plan spec (task 24 is the execution adapter); wiring must own this. (b) `pending_plan_events` is deliberately transient: a host that persists between a fill and the next `synchronize_plan_execution` loses the fill fact for the PlanBook (parent filled_qty persists; plan filled_qty stales). Task 27's save contract must require a sync before persisting session+PlanBook together. (c) A host-applied terminal event (direct `PlanBook.apply(Terminated/…)`) while a linked child is in flight would make the day-end `DayEnded` illegal and permanently fail `synchronize_plan_execution` (pending queue never clears). Unreachable through this commit's own API (no internal path terminates a plan while its parent persists); task 26 must route terminations through child-cancel first.
   Minor test-coverage gaps, same category: the cancel-conflicting-working-orders-then-submit branch inside `submit_plan_child` (and its non-cancelable `PendingReconsideration` sub-branch) and the `UnconvertedFractionTarget` rejection are not directly exercised — each is a simple composition of individually tested pieces.

## Claim cross-check summary

Worker claims focused 12/12, full 939/0/4, clippy all-targets red pre-existing from tasks 17/20 — **all three independently reproduced by reviewer with matching numbers**; evidence files (task-24-happy.txt, task-24-failure.txt, task-24-fullsuite-raw.txt) accurately describe the runs. No misleading success found.

## Conclusion

The commit does exactly what task 24 requires: plans 21-23 outputs execute through the real order lifecycle with one in-flight child per account+stock, honest queue-priority semantics, real fills as the only progress driver, and correct partial-fill / auction-carry / closing-auction / day-end release behavior. All A-share semantic seams mirror existing authoritative code. Findings 8's items are forward-looking wiring contracts, none blocking. Approve.
