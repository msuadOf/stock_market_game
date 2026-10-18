//! 语义矩阵缺口测试草案（R23/R24/R30/R33）——引擎 worktree 占用期间的备产。
//!
//! 依据：`agents/std-thread-migration/plans/haiku-traceability-refresh.md`
//! （2026-10-18 刷新结论：R30/R33/R23/R24 四面部分覆盖）与
//! `agents/std-thread-migration/plans/semantic-traceability.md`（规则定义）。
//! 编号澄清：任务书把「计划分类观察节奏」标作 R23、「定时自动」标作 R24；
//! 按两份矩阵文档，前者=R22b（refresh 判已覆盖，factory.rs 有锚）、后者=R30。
//! 本草案四面=R23 受理先后 / R24 计划根逐个完成与 terminal 生命周期 /
//! R30 定时自动（引擎面）/ R33 尚未模拟负向范围。
//!
//! 使用方式：本文件不可直接编译（多节分属不同目标文件）。落地时按各节
//! 头注释的「落地文件」整节拷入；标注「落地核对」处为未运行 cargo 的
//! 只读约束下需现场确认的签名点。配套说明见 `.tmp/gap-test-drafts.md`。

// ════════════════════════════════════════════════════════════════════
// 第 1 节　R23 受理先后（缺口：来源标签非优先级、同账户按接收事实裁决）
// 落地文件：packages/engine/src/runtime/drive_tests.rs
// （依赖 setup_fixture 的 belief NPC 位与 force_due_for_test_at 测试钩子；
//   文件头已有 `use super::bridge_tests::setup_fixture;`）
// ════════════════════════════════════════════════════════════════════

// R23 缺口 2/2：同账户现金裁决按**接收事实**（批内 FIFO 是接收先后的确定性
// 投影——player_market_and_symbolic_intents_route_into_runtime_world 已注明
// 「同批注入（批内 FIFO）」），不按订单 ID、来源标签或任何固定优先级。
//
// 手算 oracle（proposed_defaults：佣金=max(0.00025×成交额, 500 分)、过户
// 0.01‰=0.00001×成交额、连续相位空簿参考价 1000）：
// - 单 A：买 100@1000 ⇒ purse = 100_000 + max(25,500) + 1 = **100_501 分**；
// - 单 B：买 200@1000 ⇒ purse = 200_000 + max(50,500) + 2 = **200_502 分**；
// - 起始现金恰 = 200_502（只够一单）：
//   批 [A,B]（A 先接收）⇒ A 受理、B 显式 InsufficientCash，
//     期末现金 = 200_502 − 100_501 = **100_001**；
//   批 [B,A]（B 先接收）⇒ B 受理、A 拒，期末现金 = 200_502 − 200_502 = **0**。
// 两批结果随接收序翻转 ⇒ 裁决依据=接收先后（若实现按 ID/来源固定排序，
// 两批会得到同一结果，断言显红）。
#[test]
fn r23_same_account_cash_adjudicates_by_receipt_order() {
    // (reverse, want_accepted_qty, want_cash_after)：接收序翻转 ⇒ 结果翻转。
    for (reverse, want_qty, want_cash_after) in [(false, 100u32, 100_001i64), (true, 200, 0)] {
        let mut setup = setup_fixture();
        setup.npcs.retail_count = 0;
        setup.npcs.inst_count = 0; // 纯玩家 fixture：无 NPC 有机单噪声
        setup.auction_ticks = 0; // tick 1 即连续相位
        setup.config = crate::config::GameConfig {
            starting_cash: crate::money::Money::from_cents(200_502),
            ..setup.config.clone()
        };
        let mut session = GameSession::new(setup, 199).expect("会话构造");
        let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
        driver.adopt_session_seq(&session);
        let order = |qty: u32| crate::session::ReceiptBearingIntent {
            owner: crate::AccountId(0),
            intent: crate::Intent::PlaceLimit {
                code: crate::StockId(0),
                side: crate::orderbook::Side::Buy,
                price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                    1_000,
                )),
                qty,
            },
            account_ordinal: 0,
            stock_ordinal: 0,
        };
        let batch = if reverse {
            vec![order(200), order(100)]
        } else {
            vec![order(100), order(200)]
        };
        let uppers: Vec<i64> = driver
            .world_public_views()
            .iter()
            .map(|v| v.daily_upper_limit)
            .collect();
        driver.inject_player_intents(&batch, &uppers).expect("注入");
        let events = driver.step_frame_equivalent(&mut session).expect("回执拍");
        driver.install_market_views(&mut session).expect("行情");
        // 恰一受理：受理者余量=先接收单的全量（100 与 200 可判别）。
        let accepted: Vec<u32> = events
            .iter()
            .filter_map(|e| match e {
                crate::Event::OrderAccepted { remaining_qty, .. } => Some(*remaining_qty),
                _ => None,
            })
            .collect();
        assert_eq!(
            accepted,
            vec![want_qty],
            "批序翻转={reverse}：恰先到者受理（接收事实裁决）"
        );
        let rejected = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    crate::Event::IntentRejected {
                        reason: crate::RejectionReason::InsufficientCash,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(rejected, 1, "后到单显式 InsufficientCash（不静默吞）");
        let accounts = driver.world_accounts_snapshot().expect("组快照");
        assert_eq!(
            accounts[0].1[0].cash, want_cash_after,
            "purse 只扣受理单（手算 {want_cash_after} 分）"
        );
    }
}

// R23 缺口 1/2：来源标签非优先级。NPC 侧（belief 链 + force_due_for_test_at
// 定时决策）与玩家侧（inject_player_intents）**同拍**申报同一股票：
// (a) 双方都按入口事实受理/成交——来源不产生拒收或静默丢弃（铁律二）；
// (b) 同股内 NPC 受理事件 seq 严格先于玩家受理 seq（受理先后=股入口实际
//     发生先后；当前实现里组合向量 [NPC 块, 玩家块] 即入口序——任何按
//     来源标签/订单 ID 的重排都会翻转 seq 序而显红）；
// (c) 同价竞争的**赢家不设断言**（tick 内任意=既定策略，见
//     project_same_price_winner_policy），只锚受理面。
// 落地核对：belief 位在暖机后 force_due 是否必产单（种子相关）——若
// npc_accepted 前置断言红，换种子；该断言故意大声失败（fixture 失效不
// 静默绿）。
#[test]
fn r23_cross_source_acceptance_follows_entry_not_label() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 2;
    setup.strategy_params.inst.order_size = 100;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 51).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 暖机 3 拍（belief 链需要观察历史；solmid_echo 同款）。
    for _ in 0..3 {
        driver.step_frame_equivalent(&mut session).expect("暖机");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    // NPC 侧：强制 belief 位（local≥2）在下一拍到期决策（sol4 同款钩子）。
    let belief_ids: Vec<crate::AccountId> = driver.id_map.iter().skip(2).copied().collect();
    assert!(!belief_ids.is_empty(), "fixture 须有 belief NPC（inst_count=2）");
    driver.force_due_for_test_at(&belief_ids, driver.world_committed_tick() + 1);
    // 玩家侧：同股（stock 0）买 100@1000——与 belief 单同拍申报。
    let uppers: Vec<i64> = driver
        .world_public_views()
        .iter()
        .map(|v| v.daily_upper_limit)
        .collect();
    driver
        .inject_player_intents(
            &[crate::session::ReceiptBearingIntent {
                owner: crate::AccountId(0),
                intent: crate::Intent::PlaceLimit {
                    code: crate::StockId(0),
                    side: crate::orderbook::Side::Buy,
                    price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                        1_000,
                    )),
                    qty: 100,
                },
                account_ordinal: 0,
                stock_ordinal: 0,
            }],
            &uppers,
        )
        .expect("注入");
    let events = driver.step_frame_equivalent(&mut session).expect("同拍");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    // (a) NPC 前置：本拍 desk_lifecycle 有 belief 位的 Accepted（fixture 有效性）。
    let rec = driver.world_report_log().last().expect("报告在场");
    let npc_accepted = rec
        .desk_lifecycle
        .iter()
        .filter(|f| {
            matches!(f.kind, super::protocol::LifecycleKind::Accepted) && f.owner.1 >= 2
        })
        .count();
    assert!(
        npc_accepted >= 1,
        "fixture 失效：belief 位本拍未受理任何单（换种子或核 force_due 语义）"
    );
    // (a') 玩家单不得因来源被拒收/静默丢弃：受理（挂簿）或成交（吃单）二选一。
    let player_accounted = events.iter().any(|e| {
        matches!(
            e,
            crate::Event::OrderAccepted {
                account: crate::AccountId(0),
                ..
            }
        ) || matches!(
            e,
            crate::Event::Trade {
                taker: crate::AccountId(0),
                ..
            }
        )
    });
    assert!(
        player_accounted,
        "玩家单须受理或成交（来源标签不得拒收/丢弃）：{events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            crate::Event::IntentRejected {
                account: crate::AccountId(0),
                ..
            }
        )),
        "合法价位的玩家单不得被拒（来源性拒收回执）：{events:?}"
    );
    // (b) 同股受理 seq 序：NPC 块先于玩家块（组合序=入口序的重排检测）。
    // 仅当双方在 stock 0 都有 OrderAccepted 时才可比（玩家单吃单成交时无
    // 受理事件——该拍跳过 seq 锚，(a') 已兜住不丢弃面）。
    let player_seq = events.iter().filter_map(|e| match e {
        crate::Event::OrderAccepted {
            account: crate::AccountId(0),
            code: crate::StockId(0),
            seq,
            ..
        } => Some(*seq),
        _ => None,
    });
    let npc_seqs: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            crate::Event::OrderAccepted {
                account,
                code: crate::StockId(0),
                seq,
                ..
            } if *account != crate::AccountId(0) => Some(*seq),
            _ => None,
        })
        .collect();
    if let (Some(p_seq), true) = (player_seq.clone().max(), !npc_seqs.is_empty()) {
        assert!(
            npc_seqs.iter().all(|s| *s < p_seq),
            "同股 NPC 受理 seq {npc_seqs:?} 须先于玩家 {p_seq}（来源/ID 重排即红）"
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// 第 2 节　R24 计划根逐个完成交付 / terminal 生命周期
//（refresh 缺口：计划根逐个完成、terminal 记录生命周期未完整锚定——
//  sol4th 已锚单根 Completed；本节补「逐个完成不等同批其他根」与
//  「terminal 释槽后可再开新根」两面）
// 落地文件：packages/engine/src/runtime/group.rs 的 sol_recheck3_tests
// 模块（chain_harness_target 邻近，`use super::*` 环境——GroupState/
// GroupAccount/TickPlan/ChildTicket/order_id_of/DeskBatch/FillFact 均
//  裸名可用；落地建议：把 chain_harness_target 参数化为本节 harness_roots，
//  避免两份 RootProjection 字面量）
// ════════════════════════════════════════════════════════════════════

/// 双根 harness：1 账户（AccountId(9)）× 2 股——根 A=买 100@stock0、
/// 根 B=卖 200@stock1（shares[1]=500 备付）。结构逐字对照
/// chain_harness_target（group.rs :5030），仅 n_stocks/根数参数化。
fn harness_roots() -> (GroupState, crate::plans::PlanId, crate::plans::PlanId) {
    use crate::plans::{OpinionSource, PlanOpen, PlanOpinion, PlanTarget, Urgency};
    let mut state = GroupState::new(
        0,
        vec![GroupAccount::new(10_000_000, 2)],
        8,
        100,
        std::sync::Arc::new(crate::config::GameConfig::proposed_defaults()),
        240,
        None,
        Vec::new(),
        Box::new(|_t, _r| TickPlan::default()),
    );
    state.cur_tick = 1;
    state.cur_round = 0;
    state.root_projection = Some(std::sync::Arc::new(crate::runtime::world::RootProjection {
        // 落地核对：与 chain_harness_target :5044-5076 同款字面量——
        // 2 股时 catalog 两条 meta，其余字段不变。
        catalog: crate::stock::StockCatalog::from_metas(vec![
            crate::stock::StockMeta::new("600000"),
            crate::stock::StockMeta::new("600001"),
        ])
        .expect("目录构造"),
        company_registry: std::sync::Arc::new(
            crate::company::CompanyRegistry::new(Vec::new()).expect("空注册表"),
        ),
        operations: std::sync::Arc::new(
            crate::company::operations::CompanyOperations::new(
                crate::company::operations::CompanyOperationsConfig {
                    seed: 1,
                    shock_params: crate::company::events::ShockParams::current_default_parameters(),
                    catalog: crate::company::CompanyCatalog::from_codes(&[])
                        .expect("空公司目录"),
                    companies: Vec::new(),
                },
                crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("日期合法"),
            )
            .expect("空经营"),
        ),
        account_ids: vec![crate::AccountId(9)],
        auction_ticks: 0,
        closing_auction_ticks: 3,
        chain_params: vec![None],
        session_seed: 1,
        urgency_policy: crate::plans::UrgencyPolicy::default(),
        total_shares: Default::default(),
        max_order_qty_limit: Default::default(),
        tick_size_cents: Default::default(),
        stock_codes: Vec::new(),
    }));
    state.accounts[0].shares[1] = 500; // 根 B（卖腿）备付持仓
    let open = |code: u32, side, target| PlanOpen {
        account: crate::AccountId(9),
        code: crate::StockId(code),
        direction: side,
        target: PlanTarget::ShareCount(target),
        opinion: PlanOpinion {
            signal_score_bp: 3_000,
            source: OpinionSource::Blended,
        },
        confidence_bp: 8_000,
        urgency: Urgency::Normal,
        horizon_trading_days: 5,
        created_trading_day: 0,
    };
    let leg = |state: &mut GroupState, pid, code: u32, side, target| {
        state
            .plan_book
            .install_execution(pid, target, crate::money::Money::from_cents(1_000), 240)
            .expect("执行腿登记");
        state
            .plan_book
            .record_child_submitted(
                crate::AccountId(9),
                &crate::StockId(code),
                side,
                order_id_of(&(code as u16, 5)),
                target,
                0,
            )
            .expect("子单身份登记");
        state.child_in_flight.insert(
            (crate::AccountId(9), crate::StockId(code)),
            ChildTicket {
                qty: target,
                side,
                price: 1_000,
                order_id: Some(order_id_of(&(code as u16, 5))),
                remaining: target,
                charged_cents: 0,
            },
        );
    };
    let pid_a = state
        .plan_book
        .create(open(0, crate::orderbook::Side::Buy, 100))
        .expect("根 A 创建");
    let pid_b = state
        .plan_book
        .create(open(1, crate::orderbook::Side::Sell, 200))
        .expect("根 B 创建");
    leg(&mut state, pid_a, 0, crate::orderbook::Side::Buy, 100);
    leg(&mut state, pid_b, 1, crate::orderbook::Side::Sell, 200);
    (state, pid_a, pid_b)
}

/// batch 的带股参数化（既有 batch() 硬编码 stock:0）。
fn batch_on(stock: u16, entities: Vec<Entity>, fills: Vec<FillFact>) -> DeskBatch {
    DeskBatch {
        stock,
        tick: 1,
        round: 0,
        entities,
        fills,
        receipts: Vec::new(),
    }
}

// R24「逐个完成」：只有根 A 的子单全额成交 ⇒ A 独立到 Completed，
// 根 B 不被牵连（仍 Active、腿/在途票原样）——「计划根现逐个完成并交付，
// 无须等同批其他账户的根都完成」（trading-rules.md 计划根段）。
// 手算 oracle：A 的 fill 100/100 ⇒ filled_qty=**100**；
// B 无事实 ⇒ filled_qty=**0**、active_child_remaining_qty=Some(**200**)。
#[test]
fn r24_plan_roots_complete_individually_without_waiting_for_other_roots() {
    use crate::plans::PlanStatus;
    let (mut state, pid_a, pid_b) = harness_roots();
    state.accounts[0].cash = 1_000_000;
    // 只有根 A（stock0/serial5）的成交事实——sol4th 同款 FillFact。
    state.staging.push(batch_on(
        0,
        Vec::new(),
        vec![FillFact {
            order: (0, 5),
            owner: (0, 0),
            side: crate::orderbook::Side::Buy,
            price: 900,
            qty: 100,
            commission: 500,
            stamp_tax: 0,
            transfer_fee: 1,
        }],
    ));
    state.apply_staging(1, 0).expect("A 根全额成交正常推进");
    // A：filled==target ⇒ Completed；腿清空、票释放（sol4th 单根语义）。
    let a = state.plan_book.plan(pid_a).expect("A 在簿");
    assert!(matches!(a.status(), PlanStatus::Completed), "A：{:?}", a.status());
    assert_eq!(a.filled_qty(), 100, "A 手算 filled=100");
    assert!(a.execution().is_none(), "A 执行腿清空");
    assert!(a.active_child_order_id().is_none(), "A 在途身份清空");
    assert!(
        !state
            .child_in_flight
            .contains_key(&(crate::AccountId(9), crate::StockId(0))),
        "A 全成票释放"
    );
    // B：不随 A 完成而终止/暂停/清腿（逐个完成的反面锚）。
    let b = state.plan_book.plan(pid_b).expect("B 在簿");
    assert!(
        matches!(b.status(), PlanStatus::Active),
        "B 仍 Active（不随 A 完成）：{:?}",
        b.status()
    );
    assert_eq!(b.filled_qty(), 0, "B 手算 filled=0");
    assert_eq!(
        b.execution().expect("B 腿在").active_child_remaining_qty(),
        Some(200),
        "B 手算腿余量=200（原样）"
    );
    assert_eq!(
        b.active_child_order_id(),
        Some(order_id_of(&(1, 5))),
        "B 在途身份原样"
    );
    assert!(
        state
            .child_in_flight
            .contains_key(&(crate::AccountId(9), crate::StockId(1))),
        "B 在途票不被 A 的完成释放"
    );
}

// R24 terminal 生命周期下沿：terminal 记录**保留在簿**（按 id 可查、状态
// 不回退），(账户,股) 槽位即时释放——同键可开新根且 PlanId 单调不复用。
// 这是「逐个完成并交付」的下游效应：完成的根不阻塞同键新计划。
#[test]
fn r24_terminal_record_persists_and_frees_slot_for_new_root() {
    use crate::plans::{PlanOpen, PlanOpinion, PlanStatus, PlanTarget, OpinionSource, Urgency};
    let (mut state, pid_a, _pid_b) = harness_roots();
    state.accounts[0].cash = 1_000_000;
    state.staging.push(batch_on(
        0,
        Vec::new(),
        vec![FillFact {
            order: (0, 5),
            owner: (0, 0),
            side: crate::orderbook::Side::Buy,
            price: 900,
            qty: 100,
            commission: 500,
            stamp_tax: 0,
            transfer_fee: 1,
        }],
    ));
    state.apply_staging(1, 0).expect("A 完成");
    // terminal 记录保留 + active 面让位。
    assert!(matches!(
        state.plan_book.plan(pid_a).expect("A 记录仍在簿").status(),
        PlanStatus::Completed
    ));
    assert!(
        state
            .plan_book
            .active_plan(crate::AccountId(9), &crate::StockId(0))
            .is_none(),
        "(账户,股) 槽位释放"
    );
    // 同键新根：create 成功、PlanId 单调、旧 terminal 记录不被覆盖。
    let pid_new = state
        .plan_book
        .create(PlanOpen {
            account: crate::AccountId(9),
            code: crate::StockId(0),
            direction: crate::orderbook::Side::Buy,
            target: PlanTarget::ShareCount(300),
            opinion: PlanOpinion {
                signal_score_bp: 2_000,
                source: OpinionSource::Fundamental,
            },
            confidence_bp: 6_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 1,
        })
        .expect("terminal 后同键可开新根");
    assert!(pid_new.0 > pid_a.0, "PlanId 单调不复用（{pid_new:?} > {pid_a:?}）");
    assert!(matches!(
        state.plan_book.plan(pid_a).expect("A 记录仍在").status(),
        PlanStatus::Completed
    ));
    assert_eq!(
        state
            .plan_book
            .active_plan(crate::AccountId(9), &crate::StockId(0))
            .expect("新根在 active 面")
            .plan_id(),
        pid_new
    );
}

// ════════════════════════════════════════════════════════════════════
// 第 3 节　R30 定时自动（引擎面）/ R33 尚未模拟负向范围——词表编译期门
// 落地文件：packages/engine/tests/scope_boundary.rs
//（文件头补 `mod common;` 与 `use engine::Event;`）
// ════════════════════════════════════════════════════════════════════

// 词表编译期门（任务 item 2 写法草案）——三层结构：
//  1) const 名称表=注册处：新增变体必须改表（登记门）；
//  2) `const _: () = assert!(…)`：类型级哨兵——表长在编译期钉死，改表
//     必须连带改断言（零运行时成本，链接期丢弃）；
//  3) const fn 穷举 match：每变体恰一臂——新增变体 ⇒ 非穷举 ⇒ 编译失败。
// 运行期测试再把 const 表与 serde 标签集对拍（序列化契约面双保险）。
const INTENT_VARIANT_TABLE: [&str; 3] = ["PlaceLimit", "PlaceMarket", "Cancel"];
const TRADING_PHASE_TABLE: [&str; 4] = [
    "CallAuction",
    "PreOpen",
    "Continuous",
    "ClosingAuction",
];
const SECURITY_CATEGORY_TABLE: [&str; 3] = ["MainBoard", "StMainBoard", "ChiNext"];
const _: () = assert!(
    INTENT_VARIANT_TABLE.len() == 3,
    "R30/R33：Intent 词表注册处——定时/两融/大宗变体=越范围，须先过 trading-rules 裁决"
);
const _: () = assert!(
    TRADING_PHASE_TABLE.len() == 4,
    "R33：相位词表注册处——停牌/临停/退整相位=越范围"
);
const _: () = assert!(
    SECURITY_CATEGORY_TABLE.len() == 3,
    "R33：类别词表注册处——科创板/北交所/退整类别=越范围"
);

/// const fn 穷举门（R30/R33）。落地核对：`&Intent` 含非 Copy 字段——
/// 模式用 `..` 不发生移动，const fn 应合法；若编译器拒绝，退化为普通
/// `fn`（穷举编译门不损失，只是失去 const 语义）。
const fn intent_variant_name_const(intent: &Intent) -> &'static str {
    match intent {
        Intent::PlaceLimit { .. } => INTENT_VARIANT_TABLE[0],
        Intent::PlaceMarket { .. } => INTENT_VARIANT_TABLE[1],
        Intent::Cancel { .. } => INTENT_VARIANT_TABLE[2],
    }
}

const fn trading_phase_name_const(p: &TradingPhase) -> &'static str {
    match p {
        TradingPhase::CallAuction => TRADING_PHASE_TABLE[0],
        TradingPhase::PreOpen => TRADING_PHASE_TABLE[1],
        TradingPhase::Continuous => TRADING_PHASE_TABLE[2],
        TradingPhase::ClosingAuction => TRADING_PHASE_TABLE[3],
    }
}

const fn security_category_name_const(c: &SecurityCategory) -> &'static str {
    match c {
        SecurityCategory::MainBoard => SECURITY_CATEGORY_TABLE[0],
        SecurityCategory::StMainBoard => SECURITY_CATEGORY_TABLE[1],
        SecurityCategory::ChiNext => SECURITY_CATEGORY_TABLE[2],
    }
}

/// 编译期注册处 ↔ 运行期词表 ↔ serde 契约三方一致（把 const 表钉进
/// 既有 intent_vocabulary_has_no_scheduled_margin_or_block_variants 的
/// 断言链）。
#[test]
fn r30_r33_const_tables_match_runtime_vocabularies() {
    use std::collections::BTreeSet;
    // Intent：const 表 == 既有穷举 helper 输名集 == serde 标签集。
    let samples = vec![
        Intent::PlaceLimit {
            code: StockId(0),
            side: engine::Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(1000)),
            qty: 100,
        },
        Intent::PlaceMarket {
            code: StockId(0),
            side: engine::Side::Buy,
            qty: 100,
        },
        Intent::Cancel {
            code: StockId(0),
            id: engine::OrderId(1),
        },
    ];
    let names: BTreeSet<&str> = samples
        .iter()
        .map(intent_variant_name_const)
        .collect();
    assert_eq!(
        names,
        INTENT_VARIANT_TABLE.iter().copied().collect(),
        "R30/R33：Intent 词表=限价/市价/撤单（const 门与运行词表一致）"
    );
    let mut tags = BTreeSet::new();
    for i in &samples {
        let v = serde_json::to_value(i).expect("Intent 可序列化");
        let tag = v
            .as_object()
            .and_then(|m| m.keys().next().cloned())
            .expect("外部标签枚举形态");
        tags.insert(tag.to_string());
    }
    assert_eq!(
        tags,
        INTENT_VARIANT_TABLE.iter().map(|s| s.to_string()).collect(),
        "serde 标签集与 const 表一致（新增变体=契约面显式变更）"
    );
    // Phase/Category：const 表 == 全变体遍历（穷举 const fn 已保证无遗漏）。
    let phases = [
        TradingPhase::CallAuction,
        TradingPhase::PreOpen,
        TradingPhase::Continuous,
        TradingPhase::ClosingAuction,
    ];
    assert_eq!(
        phases
            .iter()
            .map(trading_phase_name_const)
            .collect::<BTreeSet<_>>(),
        TRADING_PHASE_TABLE.iter().copied().collect::<BTreeSet<_>>(),
        "R33：相位词表四方封闭"
    );
    let categories = [
        SecurityCategory::MainBoard,
        SecurityCategory::StMainBoard,
        SecurityCategory::ChiNext,
    ];
    assert_eq!(
        categories
            .iter()
            .map(security_category_name_const)
            .collect::<BTreeSet<_>>(),
        SECURITY_CATEGORY_TABLE.iter().copied().collect::<BTreeSet<_>>(),
        "R33：类别词表三方封闭（无科创板/北交所/退整）"
    );
}

// R30「不写日终存档」具名锚（refresh 缺口：'存档不含该域'未见单独断言）。
// 定时自动是会话内 Web 功能，引擎日终存档没有它的域——跨一个日结的
// runtime 驱动后 save 全键递归扫描，scheduled/timer/automation 域词出现
// 即红。deny_unknown_fields 是契约门（新字段必须改 SaveSlot），
// 本测试是语义命名锚：新增此类域必须先修 trading-rules R30 登记。
// 落地核对：token 集与现存字段无碰撞（当前 SaveSlot 键面无此三词）。
#[test]
fn r30_day_end_save_carries_no_scheduled_automation_domain() {
    let (session, _driver, _events) =
        common::flip_helpers::runtime_events_with_driver(245, 703);
    assert_eq!(session.day(), 1, "前置：跨一个日结（245>240）");
    let save = session.save().expect("日结后可存档");
    let value = serde_json::to_value(&save).expect("存档可序列化");
    let mut keys = Vec::new();
    collect_object_keys(&value, &mut keys);
    assert!(!keys.is_empty(), "前置：键面非空");
    for k in keys {
        let lower = k.to_lowercase();
        assert!(
            !lower.contains("schedul") && !lower.contains("timer") && !lower.contains("automation"),
            "R30：日终存档不得含定时自动域键：{k}"
        );
    }
}

fn collect_object_keys(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, val) in m {
                out.push(k.clone());
                collect_object_keys(val, out);
            }
        }
        serde_json::Value::Array(a) => {
            for x in a {
                collect_object_keys(x, out);
            }
        }
        _ => {}
    }
}

// R33 退整/临停登记子面（refresh 缺口）：相位词表在**跨完整交易日的事件面**
// 封闭——只出现四个合法相位（无第五相位=无停复牌/盘中临停/退市整理入口；
// 与编译期门互为表里：编译门防新增变体，本锚防运行期出现表外相位）。
#[test]
fn r33_phase_vocabulary_closed_across_full_trading_days() {
    use std::collections::BTreeSet;
    let (_, events) = common::flip_helpers::runtime_events(245, 704);
    let names: BTreeSet<&str> = events
        .iter()
        .filter_map(|e| match e {
            Event::AuctionTick { phase, .. } | Event::AuctionCompleted { phase, .. } => {
                Some(trading_phase_name_const(phase))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        names,
        TRADING_PHASE_TABLE.iter().copied().collect(),
        "R33：跨日事件面只含四个合法相位（无停牌/临停/退整）"
    );
}

// ════════════════════════════════════════════════════════════════════
// 第 4 节　R33 新股无涨跌幅（登记子面）——行为锚
// 落地文件：packages/engine/src/runtime/desk_semantics_tests.rs
// ════════════════════════════════════════════════════════════════════

// R33「新股上市初期无涨跌幅限制不在范围」的行为锚：股票存在的**第一个
// 交易 tick**即受涨跌幅带（世界构造=上市时点等价；无任何按上市日数豁免
// 带校验的路径）。首拍带外 1101 拒收；带内交易照常成交。
// 手算 oracle（cage 关、@1000、主板 ±10% ⇒ 带 [900,1100]）：
// - 首拍买 1101 ⇒ skipped_orders=**1**（带外拒收=无新股豁免）；
// - tick3 买 100@1000 成交 ⇒ 买方现金 1_000_000−**100_501**
//   （对价 100_000+佣金 max(25,500)=500+过户 1）；
// - 卖方净得 1_000_000+**99_449**（100_000−佣金 500−印花 50−过户 1）。
#[test]
fn r33_price_band_enforced_from_first_tick_of_existence() {
    let script = |_g: GroupIdx| -> GroupScript {
        Box::new(|tick, _round| match tick {
            1 => place_at(1, vec![buy(0, 0, 1101, 100)]), // 上市首拍：带外拒收
            2 => place_at(2, vec![sell(1, 0, 1000, 100)]),
            3 => place_at(3, vec![buy(0, 0, 1000, 100)]),
            _ => TickPlan::default(),
        })
    };
    let mut world = World::spawn({
        let mut spec = basic_spec(1, 1, 2, Box::new(script));
        spec.config = config_with_cage(false); // 关笼子隔离变量——本测锚带不锚笼
        spec
    })
    .expect("世界构造");
    world.run_ticks(3).expect("3 tick 收敛");
    let report = world.shutdown().expect("关停");
    let desk = &report.desk_finals[0];
    assert_eq!(
        desk.skipped_orders, 1,
        "首个交易 tick 带外 1101 拒收（新股无涨跌幅豁免不存在）"
    );
    let a0 = &report.group_finals[0].accounts[0];
    assert_eq!(a0.shares[0], 1_100, "带内交易照常：得 100 股");
    assert_eq!(
        a0.cash,
        1_000_000 - 100_501,
        "买方=对价 100_000+佣金 500+过户 1（手算 100_501）"
    );
    let a1 = &report.group_finals[0].accounts[1];
    assert_eq!(
        a1.cash,
        1_000_000 + 99_449,
        "卖方净得（100_000−佣金 500−印花 50−过户 1）"
    );
}
