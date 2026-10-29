//! 分红与送转的日终候选计算；native civil 仅接现金分红与收缴，送转仍为独立候选。
//!
//! 调用前须在隔离公司状态安装 R1 名册/税账候选。这里只返回待行政回执验证的
//! 候选事实；`Paid`/`Credited` 不能在真实账户回执勾稽前安装到权威 Session。

use super::{
    AccountDividendGrossReceipt, DividendTaxStatus, ExternalDividendReceipt,
    FlatWithholdingReceipt, SessionCorporateActions,
};
use crate::{
    calendar::{CivilDate, TradingCalendar},
    company::{
        cash_dividend::{CashDividendStatus, HolderPaymentOutcome},
        cash_dividend_tax::{CashDividendTaxMode, DividendTaxLot, ExactDividendTaxAmount},
        share_registry::{
            AcquisitionSource, DayNetChange, HolderId, MovementScope, NetAcquisition,
            ShareDayRequest, ShareRestriction,
        },
        stock_distribution::{holder_credit_lots, StockDistributionKind, StockDistributionStatus},
        CompanySystem,
    },
    orderbook::AccountId,
    stock::{StockCatalog, StockId},
    Money,
};
use std::collections::{BTreeMap, BTreeSet};

/// `available_cash` 必须来自最终组账户的真实可用现金，不含在途 buy purse。
/// 任何被引用的账户都必须存在；候选不以零余额代替缺失账户。
pub(crate) struct DistributionsDayEndInput<'a> {
    pub day: CivilDate,
    pub calendar: &'a TradingCalendar,
    pub catalog: &'a StockCatalog,
    pub company_system: &'a CompanySystem,
    pub available_cash: &'a BTreeMap<AccountId, Money>,
    pub tax_mode: CashDividendTaxMode,
    pub flat_withholding_bp: Option<u32>,
    /// 调用者提供的逐事件来源事实；不得从 kind 或混合资本公积总额反推。
    pub capital_reserve_tax_sources: &'a BTreeMap<String, CapitalReserveTaxSource>,
}

/// 来源仅为外部传入的候选契约，evidence 字符串本身不是权威证明。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CapitalReserveTaxSource {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "保留股票溢价来源与资本增加额的 typed 税务证据；当前生产入口只运行现金分红候选，送转税务候选仅由测试构造",
        )
    )]
    StockPremium {
        evidence: String,
        capital_increase: crate::accounting::AccountingAmount,
    },
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "保留非股票溢价来源的 typed 拒绝路径；当前生产入口只运行现金分红候选，送转税务候选仅由测试构造",
        )
    )]
    Other { evidence: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DistributionsDayEndCandidate {
    pub actions: SessionCorporateActions,
    pub company_system: CompanySystem,
    pub intents: Vec<DistributionIntent>,
    pub sources: Vec<DistributionSource>,
    /// 仅用于校验和后续候选的资源输入；不能代替组端真实行政回执。
    pub projected_available_cash: BTreeMap<AccountId, Money>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WithholdingSource {
    FlatPayment { payment_id: String, plan_id: String },
    IndividualCollection { collection_id: String },
}

/// 金额单位为分，股份单位为股；每笔保留事件身份，税款独立退出。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DistributionIntent {
    DividendGrossCredit {
        payment_id: String,
        plan_id: String,
        account: AccountId,
        stock: StockId,
        amount: Money,
    },
    DividendWithholding {
        account: AccountId,
        stock: StockId,
        amount: Money,
        source: WithholdingSource,
    },
    /// 正向送转直接增加可用股份，不是当日买入，不创建 T+1 冻结。
    StockDistributionCredit {
        event_id: String,
        account: AccountId,
        stock: StockId,
        shares: u64,
        available_on: CivilDate,
    },
}

/// Account 与 External 来源分列；只有 Account 税前股利计入游戏投资者注入。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DistributionSource {
    SimpleSyntheticDividend {
        payment_id: String,
        plan_id: String,
        holder: HolderId,
        stock: StockId,
        gross: Money,
    },
    StockDistribution {
        event_id: String,
        holder: HolderId,
        stock: StockId,
        shares: u64,
        capital_reserve_tax_source: Option<CapitalReserveTaxSource>,
    },
}

#[derive(Debug)]
pub(crate) struct DistributionsDayEndError {
    pub day: CivilDate,
    pub event_id: String,
    pub stock: Option<StockId>,
    pub account: Option<AccountId>,
    pub source: Box<DistributionsDayEndFailure>,
}

impl std::fmt::Display for DistributionsDayEndError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "公司权益候选失败：日期{day}，事件{event_id}，证券{stock:?}，账户{account:?}：{source}",
            day = self.day,
            event_id = self.event_id,
            stock = self.stock,
            account = self.account,
            source = self.source
        )
    }
}

impl std::error::Error for DistributionsDayEndError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// 保留领域错误的数值变体，日终包装不得只留下字符串或日期。
#[derive(Debug, thiserror::Error)]
pub(crate) enum DistributionsDayEndFailure {
    #[error("{0}")]
    Invalid(String),
    #[error("账户 {account:?} 缺少最终可用现金")]
    MissingAvailableCash { account: AccountId },
    #[error("账户 {account:?} 最终可用现金为负数：{actual:?} 分")]
    NegativeAvailableCash { account: AccountId, actual: Money },
    #[error("有个人税账的资本公积转增缺少来源事实")]
    MissingCapitalReserveTaxSource,
    #[error("尚不支持送股的原登记权益个人税基与持有期关联，禁止按新股持有期计本次红利税")]
    UnsupportedPersonalStockBonus,
    #[error("尚不支持非股票溢价资本公积转增的个人税基：{evidence}")]
    NonPremiumCapitalReserveUnsupported { evidence: String },
    #[error("股票溢价来源证据为空")]
    EmptyPremiumEvidence,
    #[error("股票溢价来源覆盖金额 {actual:?} 与本次转增 {expected:?} 不一致")]
    PremiumCapitalMismatch {
        expected: crate::accounting::AccountingAmount,
        actual: crate::accounting::AccountingAmount,
    },
    #[error(transparent)]
    CashDividend(#[from] crate::company::cash_dividend::CashDividendError),
    #[error(transparent)]
    StockDistribution(#[from] crate::company::stock_distribution::StockDistributionError),
    #[error(transparent)]
    Tax(#[from] crate::company::cash_dividend_tax::DividendTaxError),
    #[error(transparent)]
    Registry(#[from] crate::company::share_registry::ShareRegistryError),
    #[error(transparent)]
    Company(#[from] crate::company::CompanySystemError),
    #[error(transparent)]
    Money(#[from] crate::money::MoneyError),
}
impl From<&str> for DistributionsDayEndFailure {
    fn from(value: &str) -> Self {
        Self::Invalid(value.into())
    }
}
impl From<String> for DistributionsDayEndFailure {
    fn from(value: String) -> Self {
        Self::Invalid(value)
    }
}

struct Context {
    day: CivilDate,
    event_id: String,
    stock: Option<StockId>,
    account: Option<AccountId>,
}
impl Context {
    fn error(&self, source: impl Into<DistributionsDayEndFailure>) -> DistributionsDayEndError {
        DistributionsDayEndError {
            day: self.day,
            event_id: self.event_id.clone(),
            stock: self.stock,
            account: self.account,
            source: Box::new(source.into()),
        }
    }
}

impl SessionCorporateActions {
    #[cfg(test)]
    pub(crate) fn distributions_day_end_candidate(
        &self,
        input: DistributionsDayEndInput<'_>,
    ) -> Result<DistributionsDayEndCandidate, DistributionsDayEndError> {
        self.prepare_distributions_candidate(input, true)
    }

    /// 现金生产片只复用现金与个人税候选，不顺带推进送转状态或股份 movement。
    pub(crate) fn cash_distributions_day_end_candidate(
        &self,
        input: DistributionsDayEndInput<'_>,
    ) -> Result<DistributionsDayEndCandidate, DistributionsDayEndError> {
        self.prepare_distributions_candidate(input, false)
    }

    fn prepare_distributions_candidate(
        &self,
        input: DistributionsDayEndInput<'_>,
        include_stock_distributions: bool,
    ) -> Result<DistributionsDayEndCandidate, DistributionsDayEndError> {
        let ctx = Context {
            day: input.day,
            event_id: "distributions-input".into(),
            stock: None,
            account: None,
        };
        match (input.tax_mode, input.flat_withholding_bp) {
            (CashDividendTaxMode::FlatWithholding, Some(rate))
                if rate <= 10_000 && self.dividend_tax_books.is_empty() => {}
            (CashDividendTaxMode::AShareIndividual | CashDividendTaxMode::Exempt, None)
                if self.flat_withholding_receipts.is_empty() => {}
            _ => return Err(ctx.error("税务模式/比例/税账/代扣回执不一致")),
        }
        for (account, cash) in input.available_cash {
            if cash.cents() < 0 {
                return Err(Context {
                    account: Some(*account),
                    ..ctx
                }
                .error(DistributionsDayEndFailure::NegativeAvailableCash {
                    account: *account,
                    actual: *cash,
                }));
            }
        }
        let mut result = DistributionsDayEndCandidate {
            actions: self.clone(),
            company_system: input.company_system.clone(),
            intents: vec![],
            sources: vec![],
            projected_available_cash: input.available_cash.clone(),
        };
        result.prepare_dividends(&input)?;
        if include_stock_distributions {
            result.prepare_stock_distributions(&input)?;
        }
        result.prepare_individual_collections(&input)?;
        Ok(result)
    }
}

impl DistributionsDayEndCandidate {
    fn balance_mut(
        &mut self,
        ctx: &Context,
        account: AccountId,
    ) -> Result<&mut Money, DistributionsDayEndError> {
        self.projected_available_cash
            .get_mut(&account)
            .ok_or_else(|| {
                Context {
                    account: Some(account),
                    day: ctx.day,
                    event_id: ctx.event_id.clone(),
                    stock: ctx.stock,
                }
                .error(DistributionsDayEndFailure::MissingAvailableCash { account })
            })
    }

    fn prepare_dividends(
        &mut self,
        input: &DistributionsDayEndInput<'_>,
    ) -> Result<(), DistributionsDayEndError> {
        for index in 0..self.actions.dividends.len() {
            let plan = self.actions.dividends[index].plan().clone();
            let mut ctx = Context {
                day: input.day,
                event_id: plan.plan_id.clone(),
                stock: input.catalog.id(&plan.stock.0),
                account: None,
            };
            let stock = ctx.stock.ok_or_else(|| ctx.error("分红证券不在目录"))?;
            let issuer = self
                .company_system
                .issuers()
                .get(&plan.issuer)
                .ok_or_else(|| ctx.error("发行人身份丢失"))?;
            if issuer.listed_stock.as_ref() != Some(&plan.stock) {
                return Err(ctx.error("发行人与证券不匹配"));
            }
            if self.actions.dividends[index].status() == &CashDividendStatus::Approved {
                if input.day > plan.announced_on {
                    return Err(ctx.error("错过分红公告日"));
                }
                if input.day == plan.announced_on {
                    self.actions.dividends[index]
                        .announce(input.day)
                        .map_err(|e| ctx.error(e))?;
                }
            }
            if input.day == plan.registered_on
                && self.actions.dividends[index].registration().is_none()
            {
                let registry = self
                    .actions
                    .registries
                    .iter_mut()
                    .find(|r| r.stock() == &plan.stock && r.issuer() == &plan.issuer)
                    .ok_or_else(|| ctx.error("登记日缺少匹配名册"))?;
                let snapshot = registry
                    .register(plan.plan_id.clone(), input.day)
                    .map_err(|e| ctx.error(e))?
                    .clone();
                self.actions.dividends[index]
                    .register(snapshot, input.calendar)
                    .map_err(|e| ctx.error(e))?;
                let gross = self.actions.dividends[index]
                    .total_gross()
                    .map_err(|e| ctx.error(e))?;
                let fact = self
                    .company_system
                    .dividend_plan_facts(&plan.issuer)
                    .map_err(|e| ctx.error(e))?
                    .into_iter()
                    .find(|f| f.plan_id == plan.plan_id)
                    .ok_or_else(|| ctx.error("缺少 Simple 分红批准事实"))?;
                if fact.total_gross != crate::accounting::AccountingAmount::from_money(gross) {
                    return Err(ctx.error("名册税前总额与 Simple 批准金额不一致"));
                }
                // 必须在登记当日冻结税基；之后普通卖出不能重建或改变已登记权利。
                for entitlement in self.actions.dividends[index]
                    .entitlements()
                    .map_err(|e| ctx.error(e))?
                {
                    let HolderId::Account(account) = entitlement.holder else {
                        continue;
                    };
                    if let Some(book) = self
                        .actions
                        .dividend_tax_books
                        .iter_mut()
                        .find(|b| b.account() == account && b.stock() == &plan.stock)
                    {
                        book.register_dividend(
                            format!("dividend-tax:{}", plan.plan_id),
                            input.day,
                            ExactDividendTaxAmount::new(
                                i128::from(plan.gross_per_share.cents()),
                                1,
                            )
                            .map_err(|e| ctx.error(e))?,
                        )
                        .map_err(|e| ctx.error(e))?;
                    }
                }
            } else if input.day > plan.registered_on
                && self.actions.dividends[index].registration().is_none()
            {
                return Err(ctx.error("错过分红登记日"));
            }
            if input.day < plan.payable_on
                || self.actions.dividends[index].status() == &CashDividendStatus::Paid
            {
                continue;
            }
            self.actions.dividends[index]
                .mark_payable(input.day)
                .map_err(|e| ctx.error(e))?;
            let payment_id = format!("dividend-payment:{}:{}", plan.plan_id, input.day);
            if self.actions.dividends[index]
                .payments()
                .iter()
                .any(|p| p.payment_id() == payment_id)
            {
                continue;
            }
            let paid: BTreeSet<_> = self.actions.dividends[index]
                .successful_payments()
                .iter()
                .map(|(h, _)| h.clone())
                .collect();
            let entitlements = self.actions.dividends[index]
                .entitlements()
                .map_err(|e| ctx.error(e))?
                .to_vec();
            let mut outcomes = vec![];
            let mut total = Money::ZERO;
            for entitlement in entitlements.iter().filter(|e| !paid.contains(&e.holder)) {
                ctx.account = match entitlement.holder {
                    HolderId::Account(a) => Some(a),
                    _ => None,
                };
                match &entitlement.holder {
                    HolderId::Account(account) => {
                        let balance = self.balance_mut(&ctx, *account)?;
                        *balance = balance.add(entitlement.gross).map_err(|e| ctx.error(e))?;
                        self.intents.push(DistributionIntent::DividendGrossCredit {
                            payment_id: payment_id.clone(),
                            plan_id: plan.plan_id.clone(),
                            account: *account,
                            stock,
                            amount: entitlement.gross,
                        });
                        let tax_status = if let Some(rate) = input.flat_withholding_bp {
                            let withheld = Money::from_cents(
                                crate::company::cash_dividend_tax::flat_withholding_cents(
                                    entitlement.gross.cents(),
                                    rate,
                                )
                                .map_err(|e| ctx.error(e))?,
                            );
                            let balance = self.balance_mut(&ctx, *account)?;
                            *balance = balance.sub(withheld).map_err(|e| ctx.error(e))?;
                            if withheld.cents() > 0 {
                                self.intents.push(DistributionIntent::DividendWithholding {
                                    account: *account,
                                    stock,
                                    amount: withheld,
                                    source: WithholdingSource::FlatPayment {
                                        payment_id: payment_id.clone(),
                                        plan_id: plan.plan_id.clone(),
                                    },
                                });
                            }
                            self.actions
                                .flat_withholding_receipts
                                .push(FlatWithholdingReceipt {
                                    payment_id: payment_id.clone(),
                                    plan_id: plan.plan_id.clone(),
                                    account: *account,
                                    paid_on: input.day,
                                    gross: entitlement.gross,
                                    rate_bp: u64::from(rate),
                                    withheld,
                                });
                            DividendTaxStatus::FlatWithholding
                        } else if let Some(book) = self
                            .actions
                            .dividend_tax_books
                            .iter_mut()
                            .find(|b| b.account() == *account && b.stock() == &plan.stock)
                        {
                            book.record_payment(
                                &format!("dividend-tax:{}", plan.plan_id),
                                payment_id.clone(),
                                input.day,
                                entitlement.gross,
                                format!("account-gross:{payment_id}"),
                            )
                            .map_err(|e| ctx.error(e))?;
                            DividendTaxStatus::IndividualPublicMarket
                        } else {
                            DividendTaxStatus::TreatmentNotConfigured
                        };
                        self.actions
                            .account_gross_receipts
                            .push(AccountDividendGrossReceipt {
                                payment_id: payment_id.clone(),
                                plan_id: plan.plan_id.clone(),
                                account: *account,
                                paid_on: input.day,
                                gross: entitlement.gross,
                                tax_status,
                            });
                    }
                    HolderId::External(name) if !name.trim().is_empty() => self
                        .actions
                        .external_receipts
                        .push(ExternalDividendReceipt {
                            payment_id: payment_id.clone(),
                            plan_id: plan.plan_id.clone(),
                            holder: entitlement.holder.clone(),
                            paid_on: input.day,
                            gross: entitlement.gross,
                            tax_status: DividendTaxStatus::TreatmentNotConfigured,
                        }),
                    _ => return Err(ctx.error("专户失权或 External 股东未具名")),
                }
                total = total.add(entitlement.gross).map_err(|e| ctx.error(e))?;
                outcomes.push(HolderPaymentOutcome::Paid {
                    holder: entitlement.holder.clone(),
                    amount: entitlement.gross,
                });
                self.sources
                    .push(DistributionSource::SimpleSyntheticDividend {
                        payment_id: payment_id.clone(),
                        plan_id: plan.plan_id.clone(),
                        holder: entitlement.holder.clone(),
                        stock,
                        gross: entitlement.gross,
                    });
            }
            self.actions.dividends[index]
                .settle(payment_id.clone(), input.day, outcomes)
                .map_err(|e| ctx.error(e))?;
            if total.cents() > 0 {
                self.company_system
                    .pay_dividend(
                        &plan.issuer,
                        &plan.plan_id,
                        &payment_id,
                        input.day,
                        crate::accounting::AccountingAmount::from_money(total),
                    )
                    .map_err(|e| ctx.error(e))?;
            }
        }
        Ok(())
    }

    fn prepare_stock_distributions(
        &mut self,
        input: &DistributionsDayEndInput<'_>,
    ) -> Result<(), DistributionsDayEndError> {
        for index in 0..self.actions.stock_distributions.len() {
            let plan = self.actions.stock_distributions[index].plan().clone();
            let mut ctx = Context {
                day: input.day,
                event_id: plan.event_id.clone(),
                stock: input.catalog.id(&plan.stock.0),
                account: None,
            };
            let stock = ctx.stock.ok_or_else(|| ctx.error("送转证券不在目录"))?;
            let issuer = self
                .company_system
                .issuers()
                .get(&plan.issuer)
                .ok_or_else(|| ctx.error("送转发行人身份丢失"))?;
            if issuer.listed_stock.as_ref() != Some(&plan.stock) {
                return Err(ctx.error("送转发行人与证券不匹配"));
            }
            if self.actions.stock_distributions[index].status()
                == &StockDistributionStatus::Approved
            {
                if input.day > plan.announced_on {
                    return Err(ctx.error("错过送转公告日"));
                }
                if input.day == plan.announced_on {
                    self.actions.stock_distributions[index]
                        .announce(input.day)
                        .map_err(|e| ctx.error(e))?;
                }
            }
            if input.day == plan.registered_on
                && self.actions.stock_distributions[index]
                    .registration()
                    .is_none()
            {
                let registry = self
                    .actions
                    .registries
                    .iter_mut()
                    .find(|r| r.stock() == &plan.stock && r.issuer() == &plan.issuer)
                    .ok_or_else(|| ctx.error("送转登记缺少匹配名册"))?;
                let snapshot = registry
                    .register(plan.event_id.clone(), input.day)
                    .map_err(|e| ctx.error(e))?
                    .clone();
                self.actions.stock_distributions[index]
                    .register(snapshot, input.calendar)
                    .map_err(|e| ctx.error(e))?;
            } else if input.day > plan.registered_on
                && self.actions.stock_distributions[index]
                    .registration()
                    .is_none()
            {
                return Err(ctx.error("错过送转登记日"));
            }
            if self.actions.stock_distributions[index].status()
                != &StockDistributionStatus::Registered
            {
                continue;
            }
            let fact = self
                .company_system
                .stock_distribution_facts(&plan.issuer)
                .map_err(|e| ctx.error(e))?
                .into_iter()
                .find(|f| f.event_id == plan.event_id)
                .ok_or_else(|| ctx.error("缺少 Simple 送转声明事实"))?;
            if fact.new_shares != plan.approved_total_new_shares
                || fact.kind != plan.kind
                || fact.approval_reference != plan.approval_reference
            {
                return Err(ctx.error("送转冻结分配与 Simple 声明不一致"));
            }
            if input.day > plan.ex_rights_on {
                return Err(ctx.error("错过送转入账日"));
            }
            if input.day != plan.ex_rights_on {
                continue;
            }
            let receipt = self.actions.stock_distributions[index]
                .receipt()
                .ok_or_else(|| ctx.error("送转缺少冻结分配回执"))?
                .clone();
            let credit_lots = holder_credit_lots(&receipt, input.day).map_err(|e| ctx.error(e))?;
            let capital_reserve_tax_source =
                if plan.kind == StockDistributionKind::CapitalReserveConversion {
                    input
                        .capital_reserve_tax_sources
                        .get(&plan.event_id)
                        .cloned()
                } else {
                    None
                };
            let personal_holder =
                credit_lots.iter().find_map(|lot| match lot.holder {
                    HolderId::Account(account)
                        if self.actions.dividend_tax_books.iter().any(|book| {
                            book.account() == account && book.stock() == &plan.stock
                        }) =>
                    {
                        Some(account)
                    }
                    _ => None,
                });
            if let Some(account) = personal_holder {
                ctx.account = Some(account);
                if plan.kind == StockDistributionKind::BonusShares {
                    return Err(
                        ctx.error(DistributionsDayEndFailure::UnsupportedPersonalStockBonus)
                    );
                }
                if plan.kind == StockDistributionKind::CapitalReserveConversion {
                    match &capital_reserve_tax_source {
                        None => {
                            return Err(ctx
                                .error(DistributionsDayEndFailure::MissingCapitalReserveTaxSource))
                        }
                        Some(CapitalReserveTaxSource::Other { evidence }) => {
                            return Err(ctx.error(
                                DistributionsDayEndFailure::NonPremiumCapitalReserveUnsupported {
                                    evidence: evidence.clone(),
                                },
                            ))
                        }
                        Some(CapitalReserveTaxSource::StockPremium {
                            evidence,
                            capital_increase,
                        }) => {
                            if evidence.trim().is_empty() {
                                return Err(
                                    ctx.error(DistributionsDayEndFailure::EmptyPremiumEvidence)
                                );
                            }
                            if *capital_increase != fact.capital_increase {
                                return Err(ctx.error(
                                    DistributionsDayEndFailure::PremiumCapitalMismatch {
                                        expected: fact.capital_increase,
                                        actual: *capital_increase,
                                    },
                                ));
                            }
                        }
                    }
                }
            }
            let mut changes = vec![];
            for lot in &credit_lots {
                ctx.account = match lot.holder {
                    HolderId::Account(a) => Some(a),
                    _ => None,
                };
                if lot.restriction != ShareRestriction::Unrestricted {
                    return Err(ctx.error("尚不支持组端长期限售，拒绝送转限售 lot"));
                }
                let key = match &lot.holder {
                    HolderId::Account(a) => {
                        self.balance_mut(&ctx, *a)?;
                        format!("account-{}", a.0)
                    }
                    HolderId::External(name) if !name.trim().is_empty() => {
                        format!("external-{name}")
                    }
                    _ => return Err(ctx.error("送转专户失权或 External 股东未具名")),
                };
                changes.push(DayNetChange {
                    holder: lot.holder.clone(),
                    change: i128::from(lot.qty),
                    acquisition: Some(NetAcquisition {
                        lot_id: format!("stock-distribution:{}:{key}", plan.event_id),
                        source: AcquisitionSource::CorporateAction {
                            event: plan.event_id.clone(),
                        },
                        restriction: lot.restriction.clone(),
                    }),
                    consume: None,
                });
                if let HolderId::Account(account) = lot.holder {
                    self.intents
                        .push(DistributionIntent::StockDistributionCredit {
                            event_id: plan.event_id.clone(),
                            account,
                            stock,
                            shares: lot.qty,
                            available_on: input.day,
                        });
                    if let Some(book) = self
                        .actions
                        .dividend_tax_books
                        .iter_mut()
                        .find(|b| b.account() == account && b.stock() == &plan.stock)
                    {
                        let tax_lot_id = format!("tax:stock-distribution:{}:{key}", plan.event_id);
                        book.record_net_day(format!("stock-distribution:{}:{}", plan.event_id, account.0), input.day, i128::from(lot.qty), Some(DividendTaxLot { id: tax_lot_id.clone(), qty: lot.qty, acquired_on: input.day, source: crate::company::cash_dividend_tax::TaxAcquisitionSource::CorporateAction { event: plan.event_id.clone() }, class: crate::company::cash_dividend_tax::TaxShareClass::PublicMarket })).map_err(|e| ctx.error(e))?;
                    }
                }
                self.sources.push(DistributionSource::StockDistribution {
                    event_id: plan.event_id.clone(),
                    holder: lot.holder.clone(),
                    stock,
                    shares: lot.qty,
                    capital_reserve_tax_source: capital_reserve_tax_source.clone(),
                });
            }
            let registry = self
                .actions
                .registries
                .iter_mut()
                .find(|r| r.stock() == &plan.stock && r.issuer() == &plan.issuer)
                .ok_or_else(|| ctx.error("送转入账缺少匹配名册"))?;
            registry
                .close_day(ShareDayRequest {
                    event_id: format!("stock-distribution:{}", plan.event_id),
                    day: input.day,
                    scope: MovementScope::NonTradingTransfer {
                        basis: plan.approval_reference.clone(),
                    },
                    changes,
                })
                .map_err(|e| ctx.error(e))?;
            self.company_system
                .record_stock_distribution_credit(
                    &plan.issuer,
                    &plan.event_id,
                    input.day,
                    receipt.approved_total_new_shares,
                )
                .map_err(|e| ctx.error(e))?;
            self.actions.stock_distributions[index]
                .mark_credited(input.day)
                .map_err(|e| ctx.error(e))?;
        }
        Ok(())
    }

    fn prepare_individual_collections(
        &mut self,
        input: &DistributionsDayEndInput<'_>,
    ) -> Result<(), DistributionsDayEndError> {
        for index in 0..self.actions.dividend_tax_books.len() {
            let book = &self.actions.dividend_tax_books[index];
            let account = book.account();
            let ctx = Context {
                day: input.day,
                event_id: format!(
                    "dividend-tax-collect:{}:{}:{}",
                    account.0,
                    book.stock().0,
                    input.day
                ),
                stock: input.catalog.id(&book.stock().0),
                account: Some(account),
            };
            let stock = ctx.stock.ok_or_else(|| ctx.error("股息税证券不在目录"))?;
            if book.settled_on() != input.day {
                return Err(ctx.error("税账未完成当日 R1 日结"));
            }
            if book
                .collections()
                .iter()
                .any(|c| c.event_id == ctx.event_id)
            {
                continue;
            }
            if book.outstanding().map_err(|e| ctx.error(e))?.numerator() == 0
                && !book.has_payment_on(input.day)
                && !book.has_disposition_on(input.day)
            {
                continue;
            }
            let cash = *self.balance_mut(&ctx, account)?;
            let collection = self.actions.dividend_tax_books[index]
                .collect_due(ctx.event_id.clone(), input.day, cash)
                .map_err(|e| ctx.error(e))?;
            *self.balance_mut(&ctx, account)? = collection.remaining_cash;
            if collection.collected.cents() > 0 {
                self.intents.push(DistributionIntent::DividendWithholding {
                    account,
                    stock,
                    amount: collection.collected,
                    source: WithholdingSource::IndividualCollection {
                        collection_id: ctx.event_id,
                    },
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "distributions_day_end_tests.rs"]
mod tests;
