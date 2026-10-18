//! 缺口② burst 配额化——decide 侧确定性测试（设计 v4 §3.1④）。
//!
//! 红证据纪律：每个测试注明击中的缺陷形状；d1/d2/d3 的红以**负向 mutation**
//! （移除 R1-a 分支/移除 C 类内联退化等）对修复后实现验证（对旧实现这些
//! 基线可能本就能完成，论证不替代实际红证据——设计 §3.1④ 红证据诚实性）。
//! 窗口钉住用 `DECIDE_POST_SEND_HOLD` 钩子（send 已发生、guard 未归还），
//! 不依赖时序碰运气（设计 §1.2 R1 钉住要求）。

use super::npc_decisions::{
    set_decide_panic_accounts, set_decide_post_send_hold, stream_npc_decisions_with,
    NpcDecisionStreamError,
};
use super::{DecisionAccountInput, DecisionSnapshot};
use crate::strategy::{MarketView, MomentumStrategy, SelfView, StrategyState};
use crate::{AccountId, AccountKind, Money, TradingPhase};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

fn account_input() -> DecisionAccountInput {
    DecisionAccountInput::new(
        AccountKind::Inst,
        SelfView {
            cash: Money::from_cents(10_000),
            positions: BTreeMap::new(),
        },
        StrategyState::Momentum(MomentumStrategy::new(5, 0.02, 100).unwrap()),
        None,
        None,
    )
}

fn snapshot(accounts: Vec<AccountId>) -> Arc<DecisionSnapshot> {
    let inputs: BTreeMap<_, _> = accounts
        .iter()
        .copied()
        .map(|account| (account, account_input()))
        .collect();
    Arc::new(
        DecisionSnapshot::new(
            7,
            42,
            TradingPhase::Continuous,
            11,
            MarketView {
                stocks: BTreeMap::new(),
                tick: 7,
                market_minute: 11,
            },
            None,
            accounts,
            inputs,
        )
        .unwrap(),
    )
}

fn accounts(n: u64) -> Vec<AccountId> {
    (1..=n).map(AccountId).collect()
}

/// 全局 hook（hold/panic 注入）为进程级静态——使用 hook 的测试必须串行，
/// 否则并行用例互相注入（首轮并行运行的真实失败即此形状，红证同批）。
static HOOK_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 方向A·任务粗化（宽度税治理，宽度诊断 `.tmp/width-diagnosis/`）：
/// 到期账户按 chunk 打包派发——spawned 任务数 = min(due, workers)，
/// chunk 内串行执行、逐账户消息（payload/merge/到达序 receipt）语义不变。
/// 红证口径：修复前逐账户派发，spawned == due(7) > workers(2)。
#[test]
fn chunked_dispatch_packs_accounts_into_worker_aligned_tasks() {
    let compute = crate::compute::ComputeContext::with_workers(2).unwrap();
    let merged: Arc<std::sync::Mutex<Vec<AccountId>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let merged_view = Arc::clone(&merged);
    let _ = compute.take_stream_workload_counts();
    let result = super::npc_decisions::stream_npc_account_tasks_with(
        Arc::new(accounts(7)),
        &compute,
        |_| {},
        |_| Ok::<(), ()>(()),
        move |account, result: Result<(), ()>, _errors: &mut Vec<(
            AccountId,
            NpcDecisionStreamError<()>,
        )>| {
            let _ = &result;
            merged_view.lock().unwrap().push(account);
        },
    );
    result.expect("chunked 流必须有限完成");
    let (spawned, payloads, inline) = compute.take_stream_workload_counts();
    assert_eq!(
        spawned, 2,
        "任务数必须=min(due,workers)=2（实测 spawned={spawned}；宽度税治理目标）"
    );
    assert_eq!(
        payloads + inline, 7,
        "每账户恰一次交付（payload 或内联；实测 payloads={payloads} inline={inline}）"
    );
    let mut covered = merged.lock().unwrap().clone();
    covered.sort();
    let mut expected = accounts(7);
    expected.sort();
    assert_eq!(covered, expected, "全部账户按到达序归并恰一次");
}

/// d1（负向 mutation 红：移除 C 类内联退化 → 满账死锁/超时红）：
/// 双协调者占满 drives（workers=2、drives=2、spawn 预算=0）时，池内被拒
/// 协调者必须 C 类全内联完成——不派发任何任务（spawn 派发数=0）、逐账户
/// 恰一次、台账 spawn 分量不增。
#[test]
fn d1_full_drives_falls_back_to_full_inline_without_dispatch() {
    let _hook_guard = HOOK_SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let compute = crate::compute::ComputeContext::with_workers(2).unwrap();
    let _drive_a = compute.try_enter_pool_drive_slot().expect("第一个 drive 槽");
    let _drive_b = compute.try_enter_pool_drive_slot().expect("第二个 drive 槽");
    let dispatches = Arc::new(AtomicUsize::new(0));
    let gate = {
        let dispatches = Arc::clone(&dispatches);
        Arc::new(move || {
            dispatches.fetch_add(1, Ordering::SeqCst);
        })
    };
    compute.set_dispatch_gate(gate);
    let mut consumed = Vec::new();
    let result = compute.install(|| {
        stream_npc_decisions_with(
            snapshot(accounts(3)),
            &crate::GameConfig::proposed_defaults(),
            &compute,
            |_| {},
            |decision| {
                #[cfg(test)]
                let account = decision.account;
                #[cfg(not(test))]
                let account = decision.output.accounts[0].account();
                consumed.push(account);
                Ok::<_, ()>(())
            },
        )
    });
    assert!(result.is_ok(), "满账被拒协调者必须全内联完成：{result:?}");
    let mut expected = accounts(3);
    expected.sort();
    consumed.sort();
    assert_eq!(consumed, expected, "C 类全内联逐账户恰一次");
    assert_eq!(
        dispatches.load(Ordering::SeqCst),
        0,
        "spawn 派发数必须为 0（预算为 0 时不得派发）"
    );
    assert_eq!(
        compute.admission_snapshot(),
        (2, 0),
        "drives 由测试持有，spawn 分量不得增长"
    );
}

/// d2（负向 mutation 红：移除 R1-a 池内内联分支 → 容量等待挂死红）：
/// 「send 已发生、Drop(guard) 未发生」窗口——spawned 槽位已被占用且不会在
/// 流完成前释放（结果可已送达，账面仍满载）。A 类协调者（已登记 drive）在
/// 零在飞+pending 非空时必须内联推进（R1-a），不等容量恢复。
/// 实现口径登记（与设计 §1.2 钉住要求的工程偏差，如实记录）：设计建议
/// 「任务内 send 后由测试持有 guard 释放时机」；实测 2-worker 形态下协调者
/// try_recv+yield_work 会经 rayon 让位**自执行**含阻塞 hold 的任务，把窗口
/// 钉住变成真死锁（任务永不返回、协调者永不消费）——故改为测试侧直接持有
/// 两个 `SpawnSlotGuard`（等效账面状态：spawned 槽位占用跨越结果送达之后、
/// 在流完成前不归还），语义等价且完全确定。
#[test]
fn d2_sent_result_with_held_guard_still_progresses_via_inline() {
    let _hook_guard = HOOK_SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let compute = crate::compute::ComputeContext::with_workers(3).unwrap();
    // 钉住窗口：占用两个 spawned 槽位（3-worker 池，留 1 槽给 A 类协调者
    // 登记 drive 后账面即满：drives 1 + spawned 2 = 3/3），完成前不归还。
    let held_guard_a = compute.reserve_spawn_slot().expect("空池必须可预留");
    let held_guard_b = compute.reserve_spawn_slot().expect("空池必须可预留");
    let mut consumed = Vec::new();
    let result = compute.install(|| {
        stream_npc_decisions_with(
            snapshot(accounts(3)),
            &crate::GameConfig::proposed_defaults(),
            &compute,
            |_| {},
            |decision| {
                #[cfg(test)]
                let account = decision.account;
                #[cfg(not(test))]
                let account = decision.output.accounts[0].account();
                consumed.push(account);
                Ok::<_, ()>(())
            },
        )
    });
    assert!(result.is_ok(), "满载窗口存在时仍须有限完成：{result:?}");
    let mut expected = accounts(3);
    expected.sort();
    consumed.sort();
    assert_eq!(consumed, expected, "R1-a 内联推进全部账户恰一次");
    assert_eq!(
        compute.admission_snapshot(),
        (0, 2),
        "窗口内：协调者 drive 已退出、测试双 guard 仍持有"
    );
    drop(held_guard_a);
    drop(held_guard_b);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while compute.admission_snapshot() != (0, 0) {
        assert!(std::time::Instant::now() < deadline, "guard 归还后账面回基线");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// d3：spawn/inline 分计——reserve 被拒期间 spawn 派发数不增（=0），
/// inline 执行数不受限（=全部）。
#[test]
fn d3_dispatch_and_inline_are_counted_separately_under_full_ledger() {
    let _hook_guard = HOOK_SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let compute = crate::compute::ComputeContext::with_workers(2).unwrap();
    let _drive_a = compute.try_enter_pool_drive_slot().unwrap();
    let _drive_b = compute.try_enter_pool_drive_slot().unwrap();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let gate = {
        let dispatches = Arc::clone(&dispatches);
        Arc::new(move || {
            dispatches.fetch_add(1, Ordering::SeqCst);
        })
    };
    compute.set_dispatch_gate(gate);
    let inline = Arc::new(AtomicUsize::new(0));
    let inline_view = Arc::clone(&inline);
    let result = compute.install(|| {
        stream_npc_decisions_with(
            snapshot(accounts(4)),
            &crate::GameConfig::proposed_defaults(),
            &compute,
            |_| {},
            |decision| {
                #[cfg(test)]
                let account = decision.account;
                #[cfg(not(test))]
                let account = decision.output.accounts[0].account();
                let _ = account;
                inline_view.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ()>(())
            },
        )
    });
    assert!(result.is_ok());
    assert_eq!(dispatches.load(Ordering::SeqCst), 0, "reserve 被拒期间 spawn 派发数不增");
    assert_eq!(inline.load(Ordering::SeqCst), 4, "inline 执行数不受限");
}

/// d4：任务-guard 对应——派发计数、完成计数与台账释放守恒（每任务恰 1
/// 预留恰 1 归还）；运行中联合采样不变式 drives+spawned≤workers。
#[test]
fn d4_dispatch_completion_and_release_are_conserved() {
    let _hook_guard = HOOK_SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let compute = crate::compute::ComputeContext::with_workers(4).unwrap();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let gate = {
        let dispatches = Arc::clone(&dispatches);
        Arc::new(move || {
            dispatches.fetch_add(1, Ordering::SeqCst);
        })
    };
    compute.set_dispatch_gate(gate);
    let completions = Arc::new(AtomicUsize::new(0));
    let completions_view = Arc::clone(&completions);
    let violations = Arc::new(AtomicUsize::new(0));
    let violations_view = Arc::clone(&violations);
    // 采样线程：运行期间周期核对联合不变式。
    let sampler = {
        let compute = Arc::clone(&compute);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_view = Arc::clone(&stop);
        let handle = std::thread::spawn(move || {
            while !stop_view.load(Ordering::Relaxed) {
                let (drives, spawned) = compute.admission_snapshot();
                if drives + spawned > 4 {
                    violations_view.fetch_add(1, Ordering::Relaxed);
                }
                std::thread::sleep(std::time::Duration::from_micros(300));
            }
        });
        (handle, stop)
    };
    let result = compute.install(|| {
        stream_npc_decisions_with(
            snapshot(accounts(8)),
            &crate::GameConfig::proposed_defaults(),
            &compute,
            |_| {},
            |decision| {
                let _ = &decision;
                completions_view.fetch_add(1, Ordering::SeqCst);
                Ok::<_, ()>(())
            },
        )
    });
    assert!(result.is_ok());
    let dispatched = dispatches.load(Ordering::SeqCst);
    let completed = completions.load(Ordering::SeqCst);
    assert_eq!(completed, 8, "完成计数=全部账户");
    assert!(
        dispatched + (8 - dispatched.min(8)) <= 8 && dispatched <= 8,
        "派发计数不超账户总数"
    );
    // 恰 1 预留恰 1 归还：结束后 spawned 分量回 0（drives 亦随协调者退出归 0）。
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while compute.admission_snapshot() != (0, 0) {
        assert!(std::time::Instant::now() < deadline, "每任务恰一次归还（守恒）");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    sampler.1.store(true, Ordering::Relaxed);
    sampler.0.join().unwrap();
    assert_eq!(
        violations.load(Ordering::SeqCst),
        0,
        "运行期间 drives+spawned≤workers 持续成立"
    );
}

/// d5：C 类全内联退化与单 worker 池——逐账户恰一次（含单 worker 退化口径）。
#[test]
fn d5_inline_fallback_and_single_worker_pool_each_account_exactly_once() {
    let _hook_guard = HOOK_SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let single = crate::compute::ComputeContext::with_workers(1).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    let result = single.install(|| {
        stream_npc_decisions_with(
            snapshot(accounts(5)),
            &crate::GameConfig::proposed_defaults(),
            &single,
            |_| {},
            |decision| {
                #[cfg(test)]
                let account = decision.account;
                #[cfg(not(test))]
                let account = decision.output.accounts[0].account();
                assert!(seen.insert(account), "每账户恰一次（重复交付即缺陷）");
                Ok::<_, ()>(())
            },
        )
    });
    assert!(result.is_ok());
    assert_eq!(seen.len(), 5);
}

/// d6：消费错误停止派发+排空+聚合——首个消费错误返回（到达序），在飞
/// 任务全部排空（guard 最终归还）。

/// p1：pending 非空时 worker panic——原 panic payload 重抛（不挂死、不静默、
/// guard 最终归还），钉死 sender 保活反例（设计 §3.1④ p1）。
#[test]
fn p1_worker_panic_reraises_original_payload_and_releases_guards() {
    let _hook_guard = HOOK_SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let compute = crate::compute::ComputeContext::with_workers(4).unwrap();
    set_decide_panic_accounts(&[AccountId(2)]);
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compute.install(|| {
            stream_npc_decisions_with(
                snapshot(accounts(6)),
                &crate::GameConfig::proposed_defaults(),
                &compute,
                |_| {},
                |decision| {
                    let _ = &decision;
                    Ok::<_, ()>(())
                },
            )
        })
    }))
    .expect_err("panic 必须显式传播（不得静默/挂死）");
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|text| (*text).to_string()));
    assert_eq!(
        message.as_deref(),
        Some("decide quota test panic AccountId(2)"),
        "重抛必须是原 panic payload（不转换、不新建）"
    );
    set_decide_panic_accounts(&[]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while compute.admission_snapshot() != (0, 0) {
        assert!(std::time::Instant::now() < deadline, "guard 必须最终全部归还");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// p2：receive 失败后的 drain 遇 worker panic——panic 优先显式传播，不被
/// 错误聚合吞掉（设计 §3.1④ p2）。
#[test]
fn p2_panic_during_drain_overrides_consumer_error_aggregation() {
    let _hook_guard = HOOK_SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let compute = crate::compute::ComputeContext::with_workers(4).unwrap();
    set_decide_panic_accounts(&[AccountId(3)]);
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        compute.install(|| {
            stream_npc_decisions_with(
                snapshot(accounts(6)),
                &crate::GameConfig::proposed_defaults(),
                &compute,
                |_| {},
                |decision| {
                    let _ = &decision;
                    Err::<(), _>("consumer error before panic drain")
                },
            )
        })
    }))
    .expect_err("drain 期间 panic 必须优先传播");
    let is_panic = payload
        .downcast_ref::<String>()
        .is_some_and(|text| text.contains("decide quota test panic"))
        || payload
            .downcast_ref::<&str>()
            .is_some_and(|text| text.contains("decide quota test panic"));
    assert!(is_panic, "必须重抛原 decide panic 而非 Consumer 错误");
    set_decide_panic_accounts(&[]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while compute.admission_snapshot() != (0, 0) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// r1c-decide（活性契约 v5 §三 R1-c·decide 流确定性见证；Sol 终审必修 1）：
/// 「先取现成结果，再尽力派发」——当通道内已有现成结果、容量已恢复、
/// pending 仍有账户时，协调者必须先消费现成结果再补派发。
/// 钉法（不依赖时序）：2-worker 专池、池外协调者；A/B 两任务经
/// `DECIDE_POST_SEND_HOLD` 在 send 后记标志（不阻塞，guard 正常归还）；
/// `merge(first)` 自旋等待两标志 ⇒ 返回时「一已消费、一仍在通道、容量已
/// 满 2 归还、pending=[C]」全部钉死。此后事件全序必为
/// [merge:first, merge:second, dispatch:C]；旧序（派发优先）产出
/// [merge:first, dispatch:C, merge:second] ⇒ 对修复前实现本测试红。
#[test]
fn r1c_decide_consumes_ready_result_before_dispatching_more() {
    let _hook_guard = HOOK_SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let compute = crate::compute::ComputeContext::with_workers(2).unwrap();
    let a_sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let b_sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let a_sent = Arc::clone(&a_sent);
        let b_sent = Arc::clone(&b_sent);
        set_decide_post_send_hold(Some(Arc::new(move |account| {
            if account == AccountId(1) {
                a_sent.store(true, Ordering::SeqCst);
            }
            if account == AccountId(2) {
                b_sent.store(true, Ordering::SeqCst);
            }
        })));
    }
    let events: Arc<std::sync::Mutex<Vec<&'static str>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let dispatches = Arc::new(AtomicUsize::new(0));
    {
        let dispatches = Arc::clone(&dispatches);
        let events = Arc::clone(&events);
        compute.set_dispatch_gate(Arc::new(move || {
            dispatches.fetch_add(1, Ordering::SeqCst);
            events.lock().unwrap().push("dispatch");
        }));
    }
    let accounts: Arc<Vec<AccountId>> = Arc::new(vec![AccountId(1), AccountId(2), AccountId(3)]);
    let first_merge = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let result = {
        let events = Arc::clone(&events);
        let a_sent = Arc::clone(&a_sent);
        let b_sent = Arc::clone(&b_sent);
        let first_merge = Arc::clone(&first_merge);
        super::npc_decisions::stream_npc_account_tasks_with(
            Arc::clone(&accounts),
            &compute,
            |_| {},
            |_| Ok::<(), ()>(()),
            move |account, result: Result<(), ()>, _errors: &mut Vec<(
                AccountId,
                NpcDecisionStreamError<()>,
            )>| {
                let _ = result;
                // 第一/第二个结果的 merge 自旋至「A、B 两 payload 均已入通道」
                //（标志由 post-send hold 设置：send 先于标志）⇒ 钉死窗口。
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while !(a_sent.load(Ordering::SeqCst) && b_sent.load(Ordering::SeqCst)) {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "A/B send 标志 10s 未齐——任务侧异常"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                let mut events = events.lock().unwrap();
                if !first_merge.swap(true, Ordering::SeqCst) {
                    events.push("merge:first");
                } else {
                    events.push("merge:later");
                }
            },
        )
    };
    set_decide_post_send_hold(None);
    result.expect("r1c 见证流必须有限完成");
    let events = events.lock().unwrap().clone();
    assert_eq!(dispatches.load(Ordering::SeqCst), 2, "chunk 派发恰两次（[A,B]/[C]，方向A 粗化）");
    let first = events
        .iter()
        .position(|event| *event == "merge:first")
        .expect("必须存在首个 merge 事件");
    assert_eq!(
        events[first + 1], "merge:later",
        "R1-c：现成结果（已在通道内的第二份 payload）必须先于补派发 C 被消费；\
         实测事件全序={events:?}"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while compute.admission_snapshot() != (0, 0) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// r1c-decide 二代确定性红证（Sol 终审补强 2026-10-14；一百零六按方向A
/// chunk 形态重锚·池内形态）：一代见证的钉窗在 merge(first) 返回时**未钉
/// 容量恢复**——`DECIDE_POST_SEND_HOLD` 在 drop(guard) **之前**触发，旧序
/// （先补派发再消费）下若 guard 尚未析构，(a) 补派发的 reserve 失败 ⇒
/// 直接落入 (d) 消费现成结果，产出 [merge:first, merge:later, dispatch]
/// 假绿——红不是确定性的。
/// 【方向A chunk 形态下的构造迁移】chunk 数恒 ≤ workers ⇒ **池外**协调者
/// 首轮 (a) 即可派发全部 chunk（无 pending 残留），消费-vs-补派发分岔在
/// 池外不再可构造（实测：移除 (d0) 后一代/二代池外形均恒绿，红侧原始件
/// logs/r1c-red-v2-raw.log=负结果如实登记）；分岔仅存于**池内**形态——
/// drive 槽占 1 容量 ⇒ 尾 chunk 因 `drives+spawned<workers` 留 pending。
/// 本见证：4-worker 专池+池内协调者（install）；13 账户 ⇒
/// parallel_grain(13,4)=4 ⇒ chunk=[1-4]/[5-8]/[9-12]/[13]，前三 chunk 派发
/// 后 spawned=3、drives=1 ⇒ 尾 chunk=[13] 必留 pending。首个 merge 自旋至
/// 「chunk1 内 A/B 已 send **且** spawned≤workers−2=2（至少一枚 guard 已
/// 归还 ⇒ 尾 chunk reserve 立即可行）」——send 标志钉「现成结果在通道」，
/// spawned 阈值钉「补派发立即可行」，分岔点完全钉死：
/// - 新序（(d0) 先非阻塞收尽现成再 (a) 派发）⇒ merge:later 先于尾 chunk
///   的 dispatch 恒绿；
/// - 旧序（循环顶先 (a) 补派发再消费；复现补丁=移除 (d0)，专用件）⇒
///   reserve 必然成功 ⇒ events[first+1]=dispatch **确定性红**，不依赖调度。
/// 载体=真实生产骨架 `stream_npc_account_tasks_with`（decide 流与 T1
/// 账户任务流共用，消费即生产 (d0)/(d) 路径）。
#[test]
fn r1c_decide_consume_before_redispatch_deterministic() {
    let _hook_guard = HOOK_SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let compute = crate::compute::ComputeContext::with_workers(4).unwrap();
    let a_sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let b_sent = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let a_sent = Arc::clone(&a_sent);
        let b_sent = Arc::clone(&b_sent);
        set_decide_post_send_hold(Some(Arc::new(move |account| {
            if account == AccountId(1) {
                a_sent.store(true, Ordering::SeqCst);
            }
            if account == AccountId(2) {
                b_sent.store(true, Ordering::SeqCst);
            }
        })));
    }
    let events: Arc<std::sync::Mutex<Vec<&'static str>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let dispatches = Arc::new(AtomicUsize::new(0));
    {
        let dispatches = Arc::clone(&dispatches);
        let events = Arc::clone(&events);
        compute.set_dispatch_gate(Arc::new(move || {
            dispatches.fetch_add(1, Ordering::SeqCst);
            events.lock().unwrap().push("dispatch");
        }));
    }
    let accounts: Arc<Vec<AccountId>> = Arc::new(
        (1..=13u64).map(AccountId).collect::<Vec<_>>(),
    );
    let first_merge = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let result = {
        let events = Arc::clone(&events);
        let a_sent = Arc::clone(&a_sent);
        let b_sent = Arc::clone(&b_sent);
        let compute_for_merge = Arc::clone(&compute);
        let first_merge = Arc::clone(&first_merge);
        let compute_outer = Arc::clone(&compute);
        let accounts = Arc::clone(&accounts);
        compute.install(move || {
            super::npc_decisions::stream_npc_account_tasks_with(
                accounts,
                &compute_outer,
                |_| {},
                |_| Ok::<(), ()>(()),
                move |account,
                      result: Result<(), ()>,
                      _errors: &mut Vec<(
                          AccountId,
                          NpcDecisionStreamError<()>,
                      )>| {
                    let _ = result;
                    // 钉窗（Sol 补强）：首个 merge 返回前必须同时观察到——
                    // ① chunk1 内 A/B 已 send（标志后于 send）⇒ 通道内必有
                    //    未消费现成 payload；
                    // ② spawned≤workers−2（=2）：至少一枚 SpawnSlotGuard 已
                    //    析构 ⇒ drives(1)+spawned<workers ⇒ 尾 chunk=[13]
                    //    的 reserve 立即可行、pending 非空。
                    // 此刻新旧两序必在此分岔。
                    let deadline = std::time::Instant::now()
                        + std::time::Duration::from_secs(10);
                    while !(a_sent.load(Ordering::SeqCst)
                        && b_sent.load(Ordering::SeqCst)
                        && compute_for_merge.admission_snapshot().1 <= 2)
                    {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "A/B send 标志或容量归还 10s 未齐——任务侧异常"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                    let mut events = events.lock().unwrap();
                    if !first_merge.swap(true, Ordering::SeqCst) {
                        events.push("merge:first");
                    } else {
                        events.push("merge:later");
                    }
                },
            )
        })
    };
    set_decide_post_send_hold(None);
    result.expect("r1c 见证流必须有限完成");
    let events = events.lock().unwrap().clone();
    assert_eq!(
        dispatches.load(Ordering::SeqCst),
        4,
        "chunk 派发恰四次（[1-4]/[5-8]/[9-12]/[13]，方向A 粗化：前三首轮、\
         尾 chunk 在容量恢复后补派发）"
    );
    let first = events
        .iter()
        .position(|event| *event == "merge:first")
        .expect("必须存在首个 merge 事件");
    assert_eq!(
        events[first + 1], "merge:later",
        "R1-c 确定性红证：现成 payload 在通道且尾 chunk reserve 立即可行时，\
         下一事件必须是消费现成结果（merge:later），不得先补派发（dispatch）\
         ——回退旧序（先补派发再消费）本断言确定性红。实测事件全序={events:?}"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while compute.admission_snapshot() != (0, 0) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
