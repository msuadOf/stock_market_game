//! GameSession → runtime World 投影桥（批 8a：接线地基）。
//!
//! 职责：把 `SessionSetup`（**确定性配置面**：股票规格/相位参数）
//! 投影为 `WorldSpec`，使 runtime 世界与会话同构——step_frame 改接 runtime
//! 的第一步（F §4 协调者构造参数来源）。
//!
//! **本批覆盖面（诚实登记）**：setup（ticks_per_day/auction_ticks/closing/
//! order_ttl/history）、StockSpec→DeskStockSpec（交易所/初始价/板块涨跌幅/
//! 单笔上限差异）、GameConfig 直传。**未覆盖**（下一批）：
//! - 账户面投影（名册现金/持仓/T+1/在途 → GroupAccount 初值）：需要
//!   initial_allocation 与运行时账户布局的读取面；
//! - 事件流 seq/TickFrame 公共视图（协调者全局域，F §6.1）；
//! - SavedRuntimeState ↔ DayEndCandidate/三方终态映射（F §8）；
//! - 旧管线退役顺序（pipeline 执行体 → coordinator 调用替换）。

use super::decision::{DecisionSetup, NpcProfile};
use super::watchdog::WatchdogConfig;
use super::world::{DeskStockSpec, WorldSpec};
use crate::config::GameConfig;
use crate::strategy::StrategyData;
use crate::SessionSetup;
use std::sync::Arc;
use std::time::Duration;

/// 从会话确定性配置面构造 WorldSpec（决策装配与账户初值由调用方补充）。
/// 入参取 `SessionSetup`（宿主构造会话时本就持有），不耦合 GameSession
/// 私有状态面。
///
/// 防御式：股票规格非法（非正初始价）显式报错，不静默钳制。
pub fn world_spec_from_setup(
    setup: &SessionSetup,
    decision: Option<DecisionSetup>,
    script_for: Box<dyn Fn(super::GroupIdx) -> super::group::GroupScript>,
) -> Result<WorldSpec, String> {
    if setup.ticks_per_day == 0 {
        return Err("world_spec_from_setup：ticks_per_day 为零".to_owned());
    }
    let stocks: Vec<DeskStockSpec> = setup
        .stocks
        .iter()
        .map(|s| {
            let price = s.initial_price.cents();
            if price <= 0 {
                return Err(format!(
                    "world_spec_from_session：股票 {:?} 初始价非正（{price} 分）",
                    s.code.0
                ));
            }
            // 涨跌幅浮点 → 基点（对齐 desk 口径；构造期已验证为精确基点率）。
            let bps = (s.limit_pct * 10_000.0).round() as u32;
            Ok(DeskStockSpec {
                exchange: s.exchange,
                initial_price: price,
                limit_bps: bps,
                // 单笔上限按证券类别（主板/创业板 × 限价/市价差异，F §7 行 4）。
                max_order_qty_limit: s.category.max_order_qty(false),
                max_order_qty_market: s.category.max_order_qty(true),
            })
        })
        .collect::<Result<_, _>>()?;
    Ok(WorldSpec {
        n_groups: 1, // 缺省单组；组数按名册规模推导由账户面接线批补
        accounts_per_group: 0,
        initial_cash: 0,
        initial_shares: Vec::new(),
        script_for,
        chunk: 512,
        watchdog: WatchdogConfig {
            normal: Duration::from_secs(30),
            long: Duration::from_secs(120),
        },
        rounds_cap: 8,
        stocks,
        config: Arc::new(GameConfig::proposed_defaults()),
        ticks_per_day: setup.ticks_per_day,
        auction_ticks: setup.auction_ticks,
        closing_auction_ticks: setup.closing_auction_ticks,
        order_ttl_ticks: 1_000_000,
        // Sol 二审修复集 6：history_len 从会话 setup 贯通（此前硬编码
        // 16——会话设 32 时 desk 价格窗只给 16 条，恢复校验即失败）。
        history_len: setup.history_len,
        decision,
        rights_computer: None,
        initial_cash_override: None,
        plan_factory: None,
    })
}

/// 决策装配缺省：全 retail 侧写（账户面接线前的最小可跑形态）。
pub fn default_retail_decision(count: usize, seed_base: u64) -> DecisionSetup {
    DecisionSetup {
        profiles: (0..count)
            .map(|i| NpcProfile {
                strategy: StrategyData::retail(0.30, 200, 0.35),
                seed: seed_base.wrapping_add(i as u64),
                next_due: (i % 8) as u64,
            })
            .collect(),
        attention_gap_ticks: 4,
    }
}

/// 账户面投影（批 8c：step_frame 切换前置）：会话账户 → WorldSpec 账户初值
/// （现金/持仓按股数值矩阵）。经公开面 `account_strategy_profiles`（全 NPC id）
/// + `account(id)`（现金/持仓）读取；玩家账户无策略侧写不在其中——玩家组
/// 接线（F §6.3）时并入，登记。
///
/// 返回 (initial_cash_override, initial_shares, account_ids)：ids 供 AccountId
/// ↔ Owner 映射对齐（批 8d 存档映射）。
pub fn accounts_projection_from_session(
    session: &crate::GameSession,
    n_stocks: usize,
) -> (Vec<i64>, Vec<Vec<u32>>, Vec<crate::AccountId>) {
    let profiles = session.account_strategy_profiles();
    let mut cash = Vec::with_capacity(profiles.len() + 1);
    let mut shares = Vec::with_capacity(profiles.len() + 1);
    let mut ids = Vec::with_capacity(profiles.len() + 1);
    // 玩家并入（F §6.3 资源持有者同构）：AccountId(0) 惯例位先行。
    if let Some(player) = session.account(crate::AccountId(0)) {
        let mut row = vec![0u32; n_stocks];
        for (code, position) in player.positions() {
            let idx = code.0 as usize;
            if idx < n_stocks {
                row[idx] = position.qty();
            }
        }
        cash.push(player.cash().cents());
        shares.push(row);
        ids.push(crate::AccountId(0));
    }
    for (id, _profile) in profiles {
        let Some(account) = session.account(id) else {
            continue;
        };
        let mut row = vec![0u32; n_stocks];
        for (code, position) in account.positions() {
            let idx = code.0 as usize;
            if idx < n_stocks {
                row[idx] = position.qty();
            }
        }
        cash.push(account.cash().cents());
        shares.push(row);
        ids.push(id);
    }
    (cash, shares, ids)
}
