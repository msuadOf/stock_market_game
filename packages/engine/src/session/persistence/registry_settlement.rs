//! typed registry 的完整存档来源校验；外域事实来自同一 SaveSlot。

use crate::company::share_registry::{ShareRegistry, ShareRegistryError};
use crate::session::SaveSlot;
use crate::stock::StockCatalog;

pub(super) fn validate_registry_settlement_sources(
    registries: &[ShareRegistry],
    save: &SaveSlot,
    catalog: &StockCatalog,
) -> Result<(), ShareRegistryError> {
    use crate::calendar::{CalendarExchange, CalendarPolicy, TradingCalendar};
    use crate::company::share_registry::settlement_day::{
        ShareMarketConfirmation, ShareMarketSourceContext,
    };
    use crate::company::share_registry::MovementScope;
    use crate::{AccountId, StockCode, StockExchange};
    let invalid = |detail: String| ShareRegistryError::InvalidFact { detail };
    for registry in registries {
        registry.validate()?;
    }
    if !registries.iter().any(|registry| {
        registry.receipts().iter().any(|receipt| {
            matches!(
                receipt.request.scope,
                MovementScope::SettlementDayMarket { .. }
            )
        })
    }) {
        return Ok(());
    }
    let policy = CalendarPolicy::from_parts(save.civil_clock.policy.clone())
        .map_err(|cause| invalid(format!("registry restore frozen calendar policy: {cause}")))?;
    let calendar = TradingCalendar::from_policy(policy)
        .map_err(|cause| invalid(format!("registry restore frozen calendar: {cause}")))?;
    let mut confirmations = vec![];
    for (account, rows) in &save.personal_trade_confirmations {
        if !save.snapshot.accounts.contains_key(account) {
            return Err(invalid(format!(
                "registry restore confirmation account {account:?} is absent"
            )));
        }
        for row in rows {
            let code = catalog.code(row.stock).ok_or_else(|| {
                invalid(format!(
                    "registry restore confirmation {} has unknown stock {:?}",
                    row.receipt_id, row.stock
                ))
            })?;
            confirmations.push(ShareMarketConfirmation {
                account: *account,
                receipt_id: row.receipt_id,
                stock: StockCode(code.into()),
                day: row.civil_date,
                side: row.side,
                qty: u64::from(row.quantity_shares),
            });
        }
    }
    let treasury_account = if save.setup.issuer_repurchase_enabled {
        let npc_count = u64::from(save.setup.npcs.retail_count)
            .checked_add(u64::from(save.setup.npcs.inst_count))
            .and_then(|count| count.checked_add(u64::from(save.setup.npcs.hot_count)))
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| invalid("registry restore treasury account identity overflow".into()))?;
        let account = AccountId(npc_count);
        if !save.snapshot.accounts.contains_key(&account) {
            return Err(invalid(format!(
                "registry restore treasury account {account:?} is absent"
            )));
        }
        Some(account)
    } else {
        None
    };
    for registry in registries {
        let specification = save
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == *registry.stock())
            .ok_or_else(|| {
                invalid(format!(
                    "registry restore stock {} is not in frozen setup",
                    registry.stock().0
                ))
            })?;
        let exchange = match specification.exchange {
            StockExchange::Shanghai => CalendarExchange::Sse,
            StockExchange::Shenzhen => CalendarExchange::Szse,
        };
        registry.validate_market_sources(&ShareMarketSourceContext {
            calendar: &calendar,
            exchange,
            confirmations: &confirmations,
            treasury_account,
            completed_through: save.civil_clock.settled_through,
        })?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::calendar::{CalendarExchange, CalendarPolicy, CivilDate, TradingCalendar};
    use crate::company::share_registry::settlement_day::*;
    use crate::company::share_registry::*;
    use crate::company::CompanyId;
    use crate::{
        AccountId, FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Money,
        NpcSetup, RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange,
        StockSpec, StrategyParams,
    };
    include!("../../../test-support/simple_company.rs");

    fn day(text: &str) -> CivilDate {
        CivilDate::from_iso(text).unwrap()
    }

    pub(crate) fn completed_fixture() -> (SaveSlot, Vec<ShareRegistry>) {
        completed_fixture_with_foundation(false)
    }

    pub(crate) fn completed_fixture_with_foundation(
        auto_corporate_foundation: bool,
    ) -> (SaveSlot, Vec<ShareRegistry>) {
        let mut config = simple_company_fixture!(crate; ["600001"]);
        let crate::company::config::CompanySystemConfig::Simple(simple) = &mut config else {
            unreachable!()
        };
        simple.prehistory_periods = 2;
        let setup = SessionSetup {
            company_operations: None,
            groups: vec![],
            stocks: vec![StockSpec {
                code: StockCode("600001".into()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: 1000,
                float_shares: 1000,
            }],
            npcs: NpcSetup {
                retail_count: 1,
                inst_count: 0,
                hot_count: 0,
                retail_cash_median: Money::from_cents(10_000_000),
            },
            config: GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.0,
                    order_size_mean: 100,
                    chase_prob: 0.0,
                },
                inst: InstParams {
                    margin: 0.05,
                    order_size: 100,
                },
                hot: HotParams {
                    lookback: 2,
                    trend_threshold: 0.01,
                    order_size: 100,
                },
            },
            ticks_per_day: 1,
            auction_ticks: 0,
            closing_auction_ticks: 0,
            history_len: 1,
            t1_enabled: true,
            float_allocation: FloatAllocation::random(),
            start_date: day("2026-01-06"),
            simulation_policy_id: crate::SIMULATION_POLICY_ID.into(),
            company_system: Some(config),
            dividend_tax_mode: Some(crate::company::CashDividendTaxMode::FlatWithholding),
            flat_withholding_bp: Some(1000),
            rights_offering_enabled: false,
            issuer_repurchase_enabled: false,
            par_value_per_share: Some(Money::from_cents(100)),
            auto_corporate_foundation,
            report_frequency: None,
        };
        let mut game = GameSession::new(setup, 21).unwrap();
        game.step().unwrap();
        game.end_civil_day().unwrap();
        let save = game.save().unwrap();
        assert_eq!(save.civil_clock.settled_through, Some(day("2026-01-06")));
        assert!(save
            .personal_trade_confirmations
            .values()
            .all(Vec::is_empty));
        let calendar = TradingCalendar::from_policy(
            CalendarPolicy::from_parts(save.civil_clock.policy.clone()).unwrap(),
        )
        .unwrap();
        let mut registry = ShareRegistry::new(
            StockCode("600001".into()),
            CompanyId(0),
            1000,
            day("2026-01-05"),
            vec![ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![ShareLot {
                    id: "game-original".into(),
                    qty: 1000,
                    acquired_on: day("2026-01-05"),
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "固定原始持仓事实".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            }],
        )
        .unwrap();
        registry.register("R".into(), day("2026-01-05")).unwrap();
        let request = ShareSettlementDay {
            stock: registry.stock().clone(),
            day: day("2026-01-06"),
            predecessor: registry.settlement_frontier().unwrap(),
            opening: vec![],
            actual_market_close: ConfirmedShareMarketClose {
                unit: ShareUnitIdentity::Initial,
                source: ShareMarketSource::FinalConfirmations { receipts: vec![] },
                request: ShareDayRequest {
                    event_id: "game-final-market".into(),
                    day: day("2026-01-06"),
                    scope: MovementScope::PublicMarket,
                    changes: vec![DayNetChange {
                        holder: HolderId::Account(AccountId(1)),
                        change: 0,
                        acquisition: None,
                        consume: None,
                    }],
                },
            },
            post_market_actions: vec![],
        };
        let candidate = registry
            .prepare_settlement_day(
                &request,
                &ShareMarketSourceContext {
                    calendar: &calendar,
                    exchange: CalendarExchange::Sse,
                    confirmations: &[],
                    treasury_account: None,
                    completed_through: save.civil_clock.settled_through,
                },
            )
            .unwrap();
        (save, vec![candidate])
    }

    #[test]
    fn actual_save_context_rejects_orphan_confirmation_reference() {
        let (save, mut registries) = completed_fixture();
        let catalog = save.setup.stock_catalog().unwrap();
        validate_registry_settlement_sources(&registries, &save, &catalog).unwrap();
        let mut encoded = serde_json::to_value(&registries[0]).unwrap();
        encoded["receipts"][0]["request"]["scope"]["SettlementDayMarket"]["binding"]
            ["market_source"]["FinalConfirmations"]["receipts"] =
            serde_json::json!([{ "account":"1", "receipt_id":"999" }]);
        registries[0] = serde_json::from_value(encoded).unwrap();
        assert!(validate_registry_settlement_sources(&registries, &save, &catalog).is_err());
    }

    #[test]
    fn actual_save_context_rejects_missing_completion_watermark() {
        let (mut save, registries) = completed_fixture();
        let catalog = save.setup.stock_catalog().unwrap();
        save.civil_clock.settled_through = None;
        assert!(validate_registry_settlement_sources(&registries, &save, &catalog).is_err());
    }
}
