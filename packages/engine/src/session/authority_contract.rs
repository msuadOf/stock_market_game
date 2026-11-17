//! 验证专用异步合同：动作和采样由实际宿主完成，驱动不制造存储或传输事实。
use serde_json::Value;

pub const STEPS: [&str; 17] = [
    "initial",
    "manual",
    "without-observer",
    "slow-observer",
    "archive-full",
    "blocked",
    "old-receipt",
    "bad-receipt",
    "duplicate-receipt",
    "prepare-rejected",
    "replaced",
    "old-generation",
    "new-lease",
    "old-lease",
    "prefix-failure",
    "terminal-repeat",
    "shutdown",
];

#[allow(async_fn_in_trait)]
pub trait ContractPort {
    /// 必须经过实际 actor/worker；成功归档必须来自存储完成回执。
    async fn perform(&mut self, id: &str) -> Result<(), String>;
    /// 读取真实 authority、归档和 observer；不能根据动作推造结果。
    async fn snapshot(&mut self, id: &str) -> Result<Value, String>;
}

pub async fn exercise<P: ContractPort>(port: &mut P) -> Result<Vec<Value>, String> {
    let mut rows = Vec::with_capacity(STEPS.len());
    for id in STEPS {
        port.perform(id)
            .await
            .map_err(|error| format!("{id}: {error}"))?;
        let row = port.snapshot(id).await?;
        if row.get("id").and_then(Value::as_str) != Some(id) {
            return Err(format!("{id}: 宿主采样标签不一致"));
        }
        rows.push(row);
    }
    Ok(rows)
}

/// 三端使用同一短 workload，零 NPC 避免将不同到达轨迹误当语义差异。
pub fn setup() -> super::SessionSetup {
    use crate::{
        FloatAllocation, GameConfig, HotParams, InstParams, Money, NpcSetup, RetailParams,
        SecurityCategory, StockCode, StockExchange, StockSpec, StrategyParams,
    };
    super::SessionSetup {
        company_system: None,
        dividend_tax_mode: None,
        flat_withholding_bp: None,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: None,
        auto_corporate_foundation: false,
        report_frequency: None,
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![StockSpec {
            code: StockCode("600101".into()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 100000,
            float_shares: 0,
        }],
        npcs: NpcSetup {
            retail_count: 0,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(10000000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.8,
                order_size_mean: 200,
                chase_prob: 0.4,
            },
            inst: InstParams {
                margin: 0.02,
                order_size: 500,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.01,
                order_size: 300,
            },
        },
        ticks_per_day: 4,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 1,
        t1_enabled: true,
        float_allocation: FloatAllocation::random(),
        start_date: crate::CivilDate::from_iso("2030-01-02").unwrap(),
        simulation_policy_id: crate::SIMULATION_POLICY_ID.into(),
        runtime_sync_policy: Default::default(),
    }
}
