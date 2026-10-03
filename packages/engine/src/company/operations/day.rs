//! 逐自然日推进循环（经营与信息披露）：冲击到期/采样 → 到期派发 → 行业经营流 → 次日
//! 滚动利息排队。日期严格逐日；任何类型化 `Err` 上抛（经营层内部错误不吞），
//! `PaymentFailed` 在流内捕获为业务状态。

use std::collections::{BTreeMap, BTreeSet};

use crate::calendar::CivilDate;
use crate::company::events::{
    sample_company_shock, sample_industry_shock, sample_market_shock, ActiveShock, ShockKind,
};
use crate::company::operations::config::IndustryPairView;
use crate::company::operations::core::{
    ActivatedShockRecord, CompanyDayReport, CompanyOperations, ExpiredShockRecord,
    OperatingCompany, PaymentFailureRecord,
};
use crate::company::operations::error::OperationsError;
use crate::company::operations::state::EconomyAggregates;
use crate::company::scheduler::OperatingScheduler;
use crate::company::spec::{CompanyId, IndustryId};

impl CompanyOperations {
    /// 推进一个自然日（权威经营循环）。
    pub fn advance_civil_day(
        &mut self,
        date: CivilDate,
    ) -> Result<CompanyDayReport, OperationsError> {
        OperatingDayRun::begin(self, date)?.advance()
    }
}

/// 一次自然日推进的报告生命周期；权威状态和 RNG 始终借用原 CompanyOperations。
/// Err 保留此前阶段的写入，不把经营日扩成隐含事务。
struct OperatingDayRun<'a> {
    operations: &'a mut CompanyOperations,
    date: CivilDate,
    entries_before: usize,
    activated: Vec<ActivatedShockRecord>,
    expired: Vec<ExpiredShockRecord>,
    payment_failures: Vec<PaymentFailureRecord>,
    dispatched_due: usize,
}

impl<'a> OperatingDayRun<'a> {
    fn begin(
        operations: &'a mut CompanyOperations,
        date: CivilDate,
    ) -> Result<Self, OperationsError> {
        operations.invalidate_hash_projection();
        Self::validate_date(operations, date)?;
        let entries_before = operations
            .companies
            .values()
            .map(|company| company.books.books().journal().entry_count())
            .sum();
        Ok(Self {
            operations,
            date,
            entries_before,
            activated: Vec::new(),
            expired: Vec::new(),
            payment_failures: Vec::new(),
            dispatched_due: 0,
        })
    }

    fn validate_date(
        operations: &CompanyOperations,
        date: CivilDate,
    ) -> Result<(), OperationsError> {
        if operations.next_expected != Some(date) {
            return Err(OperationsError::DateOutOfSequence {
                expected: operations.next_expected_date(),
                got: date,
            });
        }
        Ok(())
    }

    fn advance(mut self) -> Result<CompanyDayReport, OperationsError> {
        self.expire_shocks();
        self.sample_and_activate_shocks()?;
        self.dispatch_due_actions()?;
        self.advance_company_flows()?;
        self.schedule_next_interest()?;
        Ok(self.build_day_report())
    }

    fn expire_shocks(&mut self) {
        for (id, company) in &mut self.operations.companies {
            for shock in company.economy.expire_before(self.date) {
                self.expired.push(ExpiredShockRecord {
                    company: id.clone(),
                    kind: shock.kind,
                });
            }
        }
    }

    fn sample_and_activate_shocks(&mut self) -> Result<(), OperationsError> {
        let operations = &mut self.operations;
        let date = self.date;
        // 市场 → 行业（IndustryId 序）→ 公司（CompanyId 序），保持 RNG 消费顺序。
        if let Some(shock) =
            sample_market_shock(&mut operations.market_rng, date, &operations.shock_params)?
        {
            self.activated.push(ActivatedShockRecord {
                company: None,
                kind: shock.kind.clone(),
                amplitude_bp: shock.amplitude_bp,
            });
            for company in operations.companies.values_mut() {
                company.economy.activate(shock.clone());
            }
        }
        let mut industry_shocks: Vec<(IndustryId, ActiveShock)> = Vec::new();
        for (industry, rng) in &mut operations.industry_rngs {
            if let Some(shock) =
                sample_industry_shock(rng, industry, date, &operations.shock_params)?
            {
                industry_shocks.push((industry.clone(), shock));
            }
        }
        for (industry, shock) in industry_shocks {
            for (id, company) in &mut operations.companies {
                if company.spec.industry == industry {
                    self.activated.push(ActivatedShockRecord {
                        company: Some(id.clone()),
                        kind: shock.kind.clone(),
                        amplitude_bp: shock.amplitude_bp,
                    });
                    company.economy.activate(shock.clone());
                }
            }
        }
        for (id, company) in &mut operations.companies {
            if let Some(shock) =
                sample_company_shock(&mut company.rng, date, &operations.shock_params)?
            {
                self.activated.push(ActivatedShockRecord {
                    company: Some(id.clone()),
                    kind: shock.kind.clone(),
                    amplitude_bp: shock.amplitude_bp,
                });
                company.economy.activate(shock);
            }
        }
        Ok(())
    }

    fn dispatch_due_actions(&mut self) -> Result<(), OperationsError> {
        self.dispatched_due = self.operations.dispatch_due_on(self.date)?;
        Ok(())
    }

    fn advance_company_flows(&mut self) -> Result<(), OperationsError> {
        let date = self.date;
        let newly: BTreeMap<CompanyId, Vec<ActiveShock>> = self
            .operations
            .companies
            .iter()
            .map(|(id, company)| {
                (
                    id.clone(),
                    company
                        .economy
                        .active()
                        .iter()
                        .filter(|shock| shock.starts_on == date)
                        .cloned()
                        .collect(),
                )
            })
            .collect();
        let interruption_recovered: BTreeSet<CompanyId> = self
            .expired
            .iter()
            .filter(|record| matches!(record.kind, ShockKind::ProductionInterruption))
            .map(|record| record.company.clone())
            .collect();
        let CompanyOperations {
            scheduler,
            companies,
            ..
        } = &mut self.operations;
        for (id, company) in companies {
            advance_flow_day(
                company,
                date,
                newly.get(id).map(Vec::as_slice).unwrap_or(&[]),
                interruption_recovered.contains(id),
                scheduler,
                &mut self.payment_failures,
            )?;
        }
        Ok(())
    }

    fn schedule_next_interest(&mut self) -> Result<(), OperationsError> {
        let next = self.date.next()?;
        self.operations.submit_rolling_interest(next)?;
        self.operations.next_expected = Some(next);
        Ok(())
    }

    fn build_day_report(self) -> CompanyDayReport {
        let posted_entries = self
            .operations
            .companies
            .values()
            .map(|company| company.books.books().journal().entry_count())
            .sum::<usize>()
            .saturating_sub(self.entries_before);
        CompanyDayReport {
            date: self.date,
            activated: self.activated,
            expired: self.expired,
            payment_failures: self.payment_failures,
            dispatched_due: self.dispatched_due,
            posted_entries,
        }
    }
}

/// 行业配对 view 的当日输入；不保存 CompanyOperations 状态副本。
struct FlowDayContext<'a> {
    company: CompanyId,
    date: CivilDate,
    aggregates: EconomyAggregates,
    newly: &'a [ActiveShock],
    interruption_recovered: bool,
    scheduler: &'a mut OperatingScheduler,
    failures: &'a mut Vec<PaymentFailureRecord>,
    next_flow_seq: &'a mut i64,
}

impl IndustryPairView<'_> {
    fn advance_day(self, context: FlowDayContext<'_>) -> Result<(), OperationsError> {
        let FlowDayContext {
            company,
            date,
            aggregates,
            newly,
            interruption_recovered,
            scheduler,
            failures,
            next_flow_seq,
        } = context;
        match self {
            Self::Industrial(books, params) => crate::company::operations::industrial::advance_day(
                company,
                books,
                params,
                date,
                aggregates,
                newly,
                scheduler,
                failures,
                next_flow_seq,
            ),
            Self::Bank(books, params) => crate::company::operations::bank::advance_day(
                company,
                books,
                params,
                date,
                aggregates,
                newly,
                scheduler,
                failures,
                next_flow_seq,
            ),
            Self::Insurance(books, params) => crate::company::operations::insurance::advance_day(
                company,
                books,
                params,
                date,
                aggregates,
                scheduler,
                failures,
                next_flow_seq,
            ),
            Self::RealEstate(books, params) => {
                crate::company::operations::real_estate::advance_day(
                    company,
                    books,
                    params,
                    date,
                    aggregates,
                    newly,
                    interruption_recovered,
                    scheduler,
                    failures,
                    next_flow_seq,
                )
            }
        }
    }
}

fn advance_flow_day(
    company: &mut OperatingCompany,
    date: CivilDate,
    newly: &[ActiveShock],
    interruption_recovered: bool,
    scheduler: &mut OperatingScheduler,
    failures: &mut Vec<PaymentFailureRecord>,
) -> Result<(), OperationsError> {
    let aggregates = company.economy.aggregates();
    IndustryPairView::at_existing_guard(&mut company.books, &company.params, &company.spec)?
        .advance_day(FlowDayContext {
            company: company.spec.id.clone(),
            date,
            aggregates,
            newly,
            interruption_recovered,
            scheduler,
            failures,
            next_flow_seq: &mut company.next_flow_seq,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::{
        AccountingAmount, InventoryItemCode, JournalLine, LedgerAccountId, PostingSide,
    };
    use crate::company::events::ShockParams;
    use crate::company::industrial::{
        industrial_account_chart, IndustrialBooks, IndustrialConfig, OpeningInventoryItem,
    };
    use crate::company::operations::config::{FlowParams, IndustryBooks};
    use crate::company::operations::{
        CompanyOperationsConfig, IndustrialFlowParams, InsuranceFlowParams, OperatingCompanyConfig,
    };
    use crate::company::scheduler::{ScheduledAction, SchedulerRequest};
    use crate::company::{
        CompanyKind, CompanySpec, CounterpartyId, CounterpartyKind, ExternalCounterparty,
        OperatingBudget,
    };

    fn start() -> CivilDate {
        CivilDate::from_iso("2030-01-01").expect("合法 Fixture 日期")
    }

    fn amount(cents: i128) -> AccountingAmount {
        AccountingAmount::from_cents(cents)
    }

    fn fixture() -> CompanyOperationsConfig {
        let line = |code: &str, side, cents| JournalLine {
            account: LedgerAccountId(code.to_string()),
            side,
            amount: amount(cents),
        };
        let customer = CounterpartyId("CUSTOMER".to_string());
        let books = IndustrialBooks::new(IndustrialConfig {
            chart: industrial_account_chart(),
            as_of: start().prev().expect("前一自然日"),
            opening_lines: vec![
                line("1002", PostingSide::Debit, 10_000),
                line("1405", PostingSide::Debit, 100),
                line("4001", PostingSide::Credit, 10_100),
            ],
            opening_inventory: vec![OpeningInventoryItem {
                account: LedgerAccountId("1405".to_string()),
                item: InventoryItemCode("FG".to_string()),
                quantity: 1,
                cost: amount(100),
            }],
            opening_assets: Vec::new(),
            opening_debt: None,
            counterparties: vec![ExternalCounterparty {
                id: customer.clone(),
                kind: CounterpartyKind::Customer,
                name: "虚构客户".to_string(),
            }],
            budget: OperatingBudget::new(AccountingAmount::ZERO, Vec::new())
                .expect("无授信 Fixture"),
            // 显式合成税率；仅用于经营调用顺序的分金额对账。
            tax_policy: crate::accounting::TaxPolicy {
                version: 1,
                vat: crate::accounting::VatPolicy {
                    output_rate_bp: 1_300,
                    input_rate_bp: 1_300,
                    deductible_share_bp: 10_000,
                },
                income_tax: crate::accounting::IncomeTaxPolicy {
                    rate_bp: 2_500,
                    loss_carryforward_years: 5,
                },
            },
        })
        .expect("工商 Fixture 装配");
        let mut shock_params = ShockParams::current_default_parameters();
        shock_params.market_candidate_bp = 0;
        shock_params.industry_candidate_bp = 0;
        shock_params.company_candidate_bp = 0;
        CompanyOperationsConfig {
            seed: 7,
            shock_params,
            companies: vec![OperatingCompanyConfig {
                spec: CompanySpec {
                    id: CompanyId("C".to_string()),
                    name: "虚构公司".to_string(),
                    industry: IndustryId("IND".to_string()),
                    kind: CompanyKind::Industrial,
                    listed_stock: None,
                    issued_shares: 100,
                    group_parent: None,
                },
                books: IndustryBooks::Industrial(books),
                flow: FlowParams::Industrial(IndustrialFlowParams {
                    customer,
                    supplier: CounterpartyId("UNUSED-SUPPLIER".to_string()),
                    raw_item: InventoryItemCode("RAW".to_string()),
                    finished_item: InventoryItemCode("FG".to_string()),
                    raw_account: LedgerAccountId("1403".to_string()),
                    finished_account: LedgerAccountId("1405".to_string()),
                    base_daily_demand_units: 0,
                    unit_price_excl_vat: amount(100),
                    receivable_credit_days: 1,
                    raw_replenish_target_units: 0,
                    raw_unit_cost_excl_vat: amount(1),
                    daily_production_units: 0,
                    daily_conversion_cost: amount(1),
                    daily_admin_expense: amount(1),
                    bad_debt_base_bp: 0,
                    asset_impairment_fraction_bp: 0,
                }),
            }],
        }
    }

    fn insurance_params() -> FlowParams {
        FlowParams::Insurance(InsuranceFlowParams {
            policyholder: CounterpartyId("P".to_string()),
            daily_groups_base: 0,
            premium: amount(100),
            expected_claims: amount(50),
            risk_adjustment: amount(1),
            coverage_days: 1,
            claim_every_days: 1,
            claim_size: amount(1),
        })
    }

    #[test]
    fn duration_error_precedes_pair_guard_and_mismatched_dto_deserializes() {
        let mut config = fixture();
        config.companies[0].flow = insurance_params();
        let json = serde_json::to_vec(&config).expect("DTO 序列化");
        let mut restored: CompanyOperationsConfig =
            serde_json::from_slice(&json).expect("错配仍在既有 Deserialize 接受集合内");
        assert!(matches!(
            CompanyOperations::new(restored.clone(), start()),
            Err(OperationsError::KindFlowMismatch {
                kind: CompanyKind::Industrial,
                flow: "Insurance",
                ..
            })
        ));
        let FlowParams::Insurance(params) = &mut restored.companies[0].flow else {
            panic!("Fixture 为 Insurance 参数")
        };
        params.coverage_days = 0;
        assert!(matches!(
            CompanyOperations::new(restored, start()),
            Err(OperationsError::InvalidDuration {
                parameter: "coverage_days",
                days: 0,
                ..
            })
        ));
    }

    #[test]
    fn restored_pair_mismatch_keeps_prior_expiry_sampling_and_due_consumption() {
        let mut ops = CompanyOperations::new(fixture(), start()).expect("装配");
        let company = ops
            .companies
            .get_mut(&CompanyId("C".to_string()))
            .expect("公司");
        company.params = insurance_params();
        company.economy.activate(ActiveShock {
            kind: ShockKind::ProductionInterruption,
            amplitude_bp: 0,
            starts_on: start().prev().expect("前日"),
            expires_on: start().prev().expect("前日"),
        });
        ops.shock_params.market_candidate_bp = 10_000;
        let before_rng = ops.market_rng.clone();
        let json = serde_json::to_vec(&ops).expect("存档序列化");
        let mut ops: CompanyOperations = serde_json::from_slice(&json).expect("错配存档仍接受");
        assert!(matches!(
            ops.advance_civil_day(start()),
            Err(OperationsError::KindFlowMismatch {
                flow: "Insurance",
                ..
            })
        ));
        assert_ne!(ops.market_rng, before_rng);
        assert_eq!(ops.scheduler.settled_through(), Some(start()));
        assert_eq!(ops.scheduler.pending_len(), 0);
        assert_eq!(ops.next_expected, Some(start()));
        let active = ops
            .companies
            .values()
            .next()
            .expect("公司")
            .economy
            .active();
        assert!(!active
            .iter()
            .any(|shock| shock.kind == ShockKind::ProductionInterruption));
        assert!(active
            .iter()
            .any(|shock| shock.kind == ShockKind::MarketDemandShift));
    }

    #[test]
    fn restored_spec_kind_is_not_an_extra_daily_pair_guard() {
        let mut config = fixture();
        config.companies[0].spec.kind = CompanyKind::Insurance;
        assert!(matches!(
            CompanyOperations::new(config, start()),
            Err(OperationsError::KindFlowMismatch { .. })
        ));
        let mut ops = CompanyOperations::new(fixture(), start()).expect("装配");
        ops.companies.values_mut().next().expect("公司").spec.kind = CompanyKind::Insurance;
        let json = serde_json::to_vec(&ops).expect("序列化");
        let mut ops: CompanyOperations = serde_json::from_slice(&json).expect("既有存档接受集合");
        let report = ops
            .advance_civil_day(start())
            .expect("day 仅检查 books/params 配对");
        assert_eq!(report.dispatched_due, 1);
        assert_eq!(report.posted_entries, 1);
        assert_eq!(ops.scheduler.pending_len(), 0);
        assert_eq!(ops.next_expected, Some(start().next().expect("次日")));
    }

    #[test]
    fn shock_records_follow_market_then_sorted_industries_then_sorted_companies() {
        let mut config = fixture();
        let mut other = config.companies[0].clone();
        config.companies[0].spec.id = CompanyId("Z".to_string());
        config.companies[0].spec.industry = IndustryId("Z-IND".to_string());
        other.spec.id = CompanyId("A".to_string());
        other.spec.industry = IndustryId("A-IND".to_string());
        // 故意按反序装配，保护输出依据领域 id 的稳定顺序。
        config.companies.push(other);
        config.shock_params.market_candidate_bp = 10_000;
        config.shock_params.industry_candidate_bp = 10_000;
        config.shock_params.company_candidate_bp = 10_000;
        let mut ops = CompanyOperations::new(config, start()).expect("双公司装配");
        let mut expected_rngs = ops.clone();
        let market = sample_market_shock(
            &mut expected_rngs.market_rng,
            start(),
            &expected_rngs.shock_params,
        )
        .expect("合法市场参数")
        .expect("必中的市场候选");
        let mut expected = vec![ActivatedShockRecord {
            company: None,
            kind: market.kind,
            amplitude_bp: market.amplitude_bp,
        }];
        for (industry, rng) in &mut expected_rngs.industry_rngs {
            let shock = sample_industry_shock(rng, industry, start(), &expected_rngs.shock_params)
                .expect("合法行业参数")
                .expect("必中的行业候选");
            let id = expected_rngs
                .companies
                .iter()
                .find(|(_, company)| company.spec.industry == *industry)
                .map(|(id, _)| id.clone())
                .expect("每个 Fixture 行业只有一家公司");
            expected.push(ActivatedShockRecord {
                company: Some(id),
                kind: shock.kind,
                amplitude_bp: shock.amplitude_bp,
            });
        }
        for (id, company) in &mut expected_rngs.companies {
            let shock =
                sample_company_shock(&mut company.rng, start(), &expected_rngs.shock_params)
                    .expect("合法公司参数")
                    .expect("必中的公司候选");
            expected.push(ActivatedShockRecord {
                company: Some(id.clone()),
                kind: shock.kind,
                amplitude_bp: shock.amplitude_bp,
            });
        }
        let report = ops.advance_civil_day(start()).expect("经营日完成");
        assert_eq!(report.activated, expected);
        assert_eq!(
            report
                .activated
                .iter()
                .map(|record| record.company.as_ref().map(|id| id.0.as_str()))
                .collect::<Vec<_>>(),
            vec![None, Some("A"), Some("Z"), Some("A"), Some("Z")],
        );
        assert_eq!(ops.market_rng, expected_rngs.market_rng);
        assert_eq!(ops.industry_rngs, expected_rngs.industry_rngs);
        for (id, company) in &ops.companies {
            assert_eq!(company.rng, expected_rngs.companies[id].rng);
        }
    }

    #[test]
    fn wrong_date_invalidates_projection_before_rejecting_without_business_changes() {
        let mut ops = CompanyOperations::new(fixture(), start()).expect("装配");
        let original = ops.hash_projection().expect("初始 projection");
        // 人为保留旧 cache，区分日期守卫之前和之后的失效时点。
        ops.seed += 1;
        assert_eq!(ops.hash_projection().expect("旧 cache"), original);
        let bytes = serde_json::to_vec(&ops).expect("拒绝前事实");
        assert!(matches!(
            ops.advance_civil_day(start().next().expect("次日")),
            Err(OperationsError::DateOutOfSequence { .. })
        ));
        assert_eq!(serde_json::to_vec(&ops).expect("拒绝后事实"), bytes);
        assert_ne!(ops.hash_projection().expect("已失效重算"), original);
    }

    fn with_receivable(admin_expense: i128) -> CompanyOperations {
        let mut config = fixture();
        let company = &mut config.companies[0];
        let FlowParams::Industrial(params) = &mut company.flow else {
            panic!("工商参数")
        };
        params.daily_admin_expense = amount(admin_expense);
        let IndustryBooks::Industrial(books) = &mut company.books else {
            panic!("工商账套")
        };
        let sale = books
            .sell_credit(
                &params.customer,
                params.finished_item.clone(),
                1,
                amount(100),
                start(),
                start().prev().expect("前日"),
            )
            .expect("赊销 Fixture");
        let mut ops = CompanyOperations::new(config, start()).expect("装配");
        ops.submit_due(SchedulerRequest::Due {
            key: "MAT:C:FIXTURE".to_string(),
            due_date: start(),
            action: ScheduledAction::ContractMaturity {
                company: CompanyId("C".to_string()),
                reference: format!("AR:{}", sale.receivable.0),
            },
        })
        .expect("回款 due");
        ops
    }

    fn cash(ops: &CompanyOperations) -> AccountingAmount {
        ops.industrial_books(&CompanyId("C".to_string()))
            .expect("工商账套")
            .books()
            .ledger()
            .account_net_debit(&LedgerAccountId("1002".to_string()))
            .expect("现金分余额")
    }

    #[test]
    fn same_day_maturity_funds_flow_then_only_next_day_interest_is_queued() {
        let mut ops = with_receivable(10_050);
        let report = ops.advance_civil_day(start()).expect("回款先于费用");
        assert_eq!(cash(&ops), amount(63));
        assert!(report.payment_failures.is_empty());
        assert_eq!(report.dispatched_due, 2);
        assert_eq!(report.posted_entries, 2);
        let pending = ops.scheduler.pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].due_date, start().next().expect("次日"));
        assert!(matches!(
            pending[0].action,
            ScheduledAction::InterestAccrual { .. }
        ));
    }

    #[test]
    fn flow_error_retains_prior_maturity_and_does_not_queue_next_interest() {
        let mut ops = with_receivable(0);
        assert!(matches!(
            ops.advance_civil_day(start()),
            Err(OperationsError::Industrial(
                crate::company::industrial::IndustrialError::NonPositiveAmount {
                    what: "expense amount",
                    ..
                }
            ))
        ));
        assert_eq!(cash(&ops), amount(10_113));
        assert_eq!(ops.scheduler.pending_len(), 0);
        assert_eq!(ops.scheduler.settled_through(), Some(start()));
        assert_eq!(ops.next_expected, Some(start()));
    }
}
