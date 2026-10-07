use crate::company::share_registry::{
    AcquisitionSource, DayNetChange, HolderId, MovementScope, NetAcquisition, ShareDayRequest,
    ShareRegistry, ShareRestriction,
};
use crate::company::stock_distribution::{holder_credit_lots, StockDistributionBook};
use crate::{account::StockCode, orderbook::AccountId};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SessionCorporateActions {
    #[ts(type = "import(\"../../save/schema/corporate-actions\").ShareRegistry[]")]
    pub registries: Vec<ShareRegistry>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").CashDividendBook[]")]
    pub dividends: Vec<crate::company::cash_dividend::CashDividendBook>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").CashDividendTaxBook[]")]
    pub dividend_tax_books: Vec<crate::company::cash_dividend_tax::CashDividendTaxBook>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").StockDistributionBook[]")]
    pub stock_distributions: Vec<StockDistributionBook>,
    /// 拆股／缩股（股份重新计值）事件账簿；严格持久化，旧档缺失该字段显式拒绝。
    #[ts(type = "import(\"../../save/schema/corporate-actions\").ShareSplitBook[]")]
    pub share_splits: Vec<crate::company::share_split::ShareSplitBook>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").RightsOfferingBook[]")]
    pub rights_offerings: Vec<crate::company::rights_offering::RightsOfferingBook>,
    /// 盘中显式认购排队（玩家／宿主当日提交，日终划扣后转入账簿）。
    #[ts(type = "import(\"../../save/schema/corporate-actions\").QueuedRightsSubscription[]")]
    pub rights_subscription_queue: Vec<QueuedRightsSubscription>,
    /// 日终公开配售超额认购的显式拒绝回执（受理侧额度校验之后的极端竞态兜底）。
    #[ts(type = "import(\"../../save/schema/corporate-actions\").RejectedRightsSubscription[]")]
    pub rejected_rights_subscriptions: Vec<RejectedRightsSubscription>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").IssuerRepurchaseBook[]")]
    pub issuer_repurchases: Vec<crate::company::issuer_repurchase::IssuerRepurchaseBook>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").AccountDividendGrossReceipt[]")]
    pub account_gross_receipts: Vec<AccountDividendGrossReceipt>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").ExternalDividendReceipt[]")]
    pub external_receipts: Vec<ExternalDividendReceipt>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").AppliedExReferenceGroup[]")]
    pub applied_ex_reference_groups: Vec<AppliedExReferenceGroup>,
}

/// 盘中排队的显式配股认购（日内提交、当日日终划扣；随日终档持久化）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct QueuedRightsSubscription {
    pub event_id: String,
    #[ts(type = "string")]
    pub account: AccountId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    #[ts(type = "string")]
    pub requested_shares: u64,
    pub submitted_on: crate::calendar::CivilDate,
}

/// 日终对排队认购的显式拒绝回执：受理侧已按剩余公开额度校验，本回执只兜底
/// 恢复后队列与额度不一致等极端竞态——按队列序处理至额度耗尽，超出部分显式
/// 拒绝、留痕并从队列移除，日终不因单条排队失败而整日失败（幂等、可恢复）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RejectedRightsSubscription {
    pub event_id: String,
    #[ts(type = "string")]
    pub account: AccountId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    #[ts(type = "string")]
    pub requested_shares: u64,
    pub submitted_on: crate::calendar::CivilDate,
    /// 拒绝发生的日终日期（≥ 提交日）。
    pub rejected_on: crate::calendar::CivilDate,
    /// 拒绝原因（非空；当前唯一成因是公开配售剩余额度耗尽）。
    pub reason: String,
}

/// 已应用到行情前收锚的除权除息组：同一证券同一除权日只产生一个参考价，
/// 组合事实同时列出参与合计的现金分红计划、送转事件、配股事件与拆股／缩股
/// 事件。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AppliedExReferenceGroup {
    pub date: crate::calendar::CivilDate,
    pub stock: StockCode,
    pub cash_plan_ids: Vec<String>,
    pub stock_event_ids: Vec<String>,
    pub rights_event_ids: Vec<String>,
    /// 参与合计的拆股／缩股事件（严格持久化：旧档缺失该字段显式拒绝）。
    pub split_event_ids: Vec<String>,
    pub reference: crate::company::ex_reference_price::ExReferencePrice,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum DividendTaxStatus {
    IndividualPublicMarket,
    TreatmentNotConfigured,
}

/// 税务身份查询的账户身份分类。
///
/// 抽象层留白：非个人身份目前统一为 [`TaxpayerIdentity::NonIndividualPending`]
/// （企业/机构计税未实现，保持 `TreatmentNotConfigured`）；后续批次接入企业口径时，
/// 在此枚举扩展具体身份（居民企业 / 证券投资基金 / 非居民），并与
/// `company::cash_dividend_tax::DividendTaxProfile` 的既有变体对应，
/// 不实现其计税前不得把非个人身份映射为任何已实现身份。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum TaxpayerIdentity {
    /// 个人（玩家与自然人散户 NPC）：大 A 个人差别化口径的可计税身份。
    Personal,
    /// 非个人（机构、游资 NPC 及外部持有人）：企业/机构税未实现，保持 TreatmentNotConfigured。
    NonIndividualPending,
}

/// 按账户种类映射税务身份：玩家与自然人散户 NPC 属个人；机构与游资 NPC 属非个人。
/// 该映射是开局默认税籍的权威分类，宿主不得由策略风格另行推断。
pub fn taxpayer_identity_of_kind(kind: crate::account::AccountKind) -> TaxpayerIdentity {
    match kind {
        crate::account::AccountKind::Player | crate::account::AccountKind::Retail => {
            TaxpayerIdentity::Personal
        }
        crate::account::AccountKind::Inst
        | crate::account::AccountKind::Hot
        | crate::account::AccountKind::IssuerRepurchase => {
            // 发行人回购账户是公司自身：非个人身份（且其专户股份本就失权，
            // 不产生个人税事实）。
            TaxpayerIdentity::NonIndividualPending
        }
    }
}

/// 单账户在单个已登记证券上的税账状态。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AccountStockDividendTaxStatus {
    pub stock: StockCode,
    pub status: DividendTaxStatus,
}

/// 单账户的现金分红税务状态查询视图：会话税务模式、按账户种类映射的身份分类
/// 与每个已配置完整名册证券上的税账状态。只读汇总既有事实，不产生新事实。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AccountDividendTaxStatusView {
    /// 本局新选定的现金分红税务模式。
    pub mode: crate::company::cash_dividend_tax::CashDividendTaxMode,
    /// 账户的纳税人身份分类（个人 / 非个人待实现）。
    pub identity: TaxpayerIdentity,
    /// 每个已配置完整名册证券上的税账状态；无任何名册时为空。
    pub stocks: Vec<AccountStockDividendTaxStatus>,
}

/// 个人现金分红税未划收税额的原因。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum DividendTaxOutstandingCause {
    /// 截至最近日终，应纳税额已全部划收，无未清税额。
    Cleared,
    /// 已评估应纳税额超过资金账户可用现金，按可用现金部分划收；
    /// 待资金补足后由后续日终继续追缴（财税〔2012〕85号第二条）。
    InsufficientAvailableCash,
}

/// 精确分数税额的查询投影：规范非负十进制字符串分子 + 规范 u64 十进制字符串分母，
/// 序列化形态与 `ExactDividendTaxAmount` 及 Web 严格 parser 的 `ExactDividendTaxAmount` 一致。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactDividendTaxFraction {
    #[serde(with = "crate::company::share_registry::canonical_i128_decimal")]
    pub numerator: i128,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub denominator: u64,
}

/// 单账户单证券的个人现金分红税未划收状态查询视图；只汇总税账既有事实，不产生新事实。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct DividendTaxOutstandingView {
    #[ts(type = "string")]
    pub account: AccountId,
    pub stock: StockCode,
    /// 尚未划收的应纳税额（分，已约简非负分数；按每笔分红合计四舍五入到分后汇总）。
    #[ts(type = "import(\"../../save/schema/corporate-actions\").ExactDividendTaxAmount")]
    pub outstanding: ExactDividendTaxFraction,
    /// 划收时资金不足导致部分收缴（等价于当前存在未划收税额）。
    pub needs_funds: bool,
    /// 未清税额原因。
    pub cause: DividendTaxOutstandingCause,
}

fn holder_key(holder: &HolderId) -> String {
    match holder {
        HolderId::Account(account) => format!("account-{}", account.0),
        HolderId::External(name) => format!("external-{name}"),
        HolderId::IssuerTreasury => "issuer-treasury".to_owned(),
    }
}

/// 汇总非交易过户回执的股数；负数或超 `u64` 的数量显式报错，不静默取 0。
fn sum_nontrading_changes(changes: &[DayNetChange]) -> Result<u64, SessionCorporateActionsError> {
    changes.iter().try_fold(0_u64, |total, change| {
        let qty = u64::try_from(change.change).map_err(|_| {
            SessionCorporateActionsError::Invalid("非交易过户回执出现负数或溢出股数".into())
        })?;
        total.checked_add(qty).ok_or_else(|| {
            SessionCorporateActionsError::Invalid("非交易过户回执股数合计溢出".into())
        })
    })
}

/// 送转非交易过户回执在税账的日结事件 id：以回执自身事件身份加账户派生，
/// 与公开市场日结（按证券+账户+自然日）分列，避免同日两条回执在税账撞车
/// 被幂等跳过（集成修复轮 major 的根因）。
fn nontrading_tax_day_event_id(receipt_event_id: &str, account: AccountId) -> String {
    format!("{receipt_event_id}:{}", account.0)
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AccountDividendGrossReceipt {
    pub payment_id: String,
    pub plan_id: String,
    #[ts(type = "string")]
    pub account: AccountId,
    pub paid_on: crate::calendar::CivilDate,
    pub gross: crate::money::Money,
    pub tax_status: DividendTaxStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ExternalDividendReceipt {
    pub payment_id: String,
    pub plan_id: String,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").HolderId")]
    pub holder: HolderId,
    pub paid_on: crate::calendar::CivilDate,
    pub gross: crate::money::Money,
    pub tax_status: DividendTaxStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum SessionCorporateActionsError {
    #[error("公司行为股东名册配置无效：{0}")]
    Invalid(String),
}

impl SessionCorporateActions {
    pub(crate) fn process_dividends_on_day_end(
        &mut self,
        day: crate::calendar::CivilDate,
        calendar: &crate::calendar::TradingCalendar,
        issuers: &crate::company::identity::IssuerRegistry,
        company_system: &mut crate::company::CompanySystem,
        accounts: &mut super::account_book::AccountBook,
    ) -> Result<(), SessionCorporateActionsError> {
        for index in 0..self.dividends.len() {
            let plan = self.dividends[index].plan().clone();
            let status = self.dividends[index].status().clone();
            if status == crate::company::cash_dividend::CashDividendStatus::Approved {
                if day > plan.announced_on {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "分红计划 {} 错过公告日",
                        plan.plan_id
                    )));
                }
                if day == plan.announced_on {
                    self.dividends[index].announce(day).map_err(|error| {
                        SessionCorporateActionsError::Invalid(error.to_string())
                    })?;
                }
            }
            let status = self.dividends[index].status().clone();
            if day == plan.registered_on {
                if status != crate::company::cash_dividend::CashDividendStatus::Announced {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "分红计划 {} 登记日前未完成公告",
                        plan.plan_id
                    )));
                }
                let registry_index = self
                    .registries
                    .iter()
                    .position(|registry| {
                        registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                    })
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("登记日缺少匹配股东名册".into())
                    })?;
                let snapshot = self.registries[registry_index]
                    .register(plan.plan_id.clone(), day)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .clone();
                self.dividends[index]
                    .register(snapshot, calendar)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                let registered_gross = crate::accounting::AccountingAmount::from_money(
                    self.dividends[index].total_gross().map_err(|error| {
                        SessionCorporateActionsError::Invalid(error.to_string())
                    })?,
                );
                let approved_gross = company_system
                    .dividend_plan_facts(&plan.issuer)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .into_iter()
                    .find(|fact| fact.plan_id == plan.plan_id)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(
                            "登记现金分红缺少Simple批准事实".into(),
                        )
                    })?
                    .total_gross;
                if registered_gross != approved_gross {
                    return Err(SessionCorporateActionsError::Invalid(
                        "登记日税前总额与已批准Simple分红金额不一致".into(),
                    ));
                }
            } else if day > plan.registered_on && self.dividends[index].registration().is_none() {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "分红计划 {} 错过登记日",
                    plan.plan_id
                )));
            }
            if day >= plan.payable_on {
                self.dividends[index]
                    .mark_payable(day)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
            if day < plan.payable_on
                || self.dividends[index].status()
                    == &crate::company::cash_dividend::CashDividendStatus::Paid
            {
                continue;
            }
            let payment_id = format!("dividend-payment:{}:{}", plan.plan_id, day);
            if self.dividends[index]
                .payments()
                .iter()
                .any(|payment| payment.payment_id() == payment_id)
            {
                continue;
            }
            let entitlements = self.dividends[index]
                .entitlements()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            let paid_holders: BTreeSet<_> = self.dividends[index]
                .successful_payments()
                .iter()
                .map(|(holder, _)| holder.clone())
                .collect();
            let mut outcomes = Vec::new();
            let mut new_external_receipts = Vec::new();
            let mut settled_gross = crate::money::Money::ZERO;
            for entitlement in entitlements
                .iter()
                .filter(|item| !paid_holders.contains(&item.holder))
            {
                match &entitlement.holder {
                    HolderId::Account(account) => {
                        let account_state = accounts.get_mut(account).ok_or_else(|| {
                            SessionCorporateActionsError::Invalid(format!(
                                "分红股东账户 {account:?} 不存在"
                            ))
                        })?;
                        match account_state.credit_cash(entitlement.gross) {
                            Ok(()) => {
                                settled_gross =
                                    settled_gross.add(entitlement.gross).map_err(|error| {
                                        SessionCorporateActionsError::Invalid(error.to_string())
                                    })?;
                                outcomes.push(
                                    crate::company::cash_dividend::HolderPaymentOutcome::Paid {
                                        holder: entitlement.holder.clone(),
                                        amount: entitlement.gross,
                                    },
                                );
                                self.account_gross_receipts
                                    .push(AccountDividendGrossReceipt {
                                        payment_id: payment_id.clone(),
                                        plan_id: plan.plan_id.clone(),
                                        account: *account,
                                        paid_on: day,
                                        gross: entitlement.gross,
                                        tax_status: if self.dividend_tax_books.iter().any(|book| {
                                            book.account() == *account
                                                && book.stock() == &plan.stock
                                        }) {
                                            DividendTaxStatus::IndividualPublicMarket
                                        } else {
                                            DividendTaxStatus::TreatmentNotConfigured
                                        },
                                    });
                            }
                            Err(error) => outcomes.push(
                                crate::company::cash_dividend::HolderPaymentOutcome::Failed {
                                    holder: entitlement.holder.clone(),
                                    reason: error.to_string(),
                                },
                            ),
                        }
                    }
                    HolderId::External(name) if !name.trim().is_empty() => {
                        settled_gross = settled_gross.add(entitlement.gross).map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                        outcomes.push(crate::company::cash_dividend::HolderPaymentOutcome::Paid {
                            holder: entitlement.holder.clone(),
                            amount: entitlement.gross,
                        });
                        new_external_receipts.push(ExternalDividendReceipt {
                            payment_id: payment_id.clone(),
                            plan_id: plan.plan_id.clone(),
                            holder: entitlement.holder.clone(),
                            paid_on: day,
                            gross: entitlement.gross,
                            tax_status: DividendTaxStatus::TreatmentNotConfigured,
                        });
                    }
                    HolderId::External(_) => {
                        return Err(SessionCorporateActionsError::Invalid(
                            "External holder 必须具名".into(),
                        ));
                    }
                    HolderId::IssuerTreasury => {
                        return Err(SessionCorporateActionsError::Invalid(
                            "IssuerTreasury 不得取得现金股利".into(),
                        ));
                    }
                }
            }
            self.dividends[index]
                .settle(payment_id.clone(), day, outcomes)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            self.external_receipts.extend(new_external_receipts);
            if settled_gross.cents() > 0 {
                let issuer = issuers.get(&plan.issuer).ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("分红付款的发行人身份丢失".into())
                })?;
                let settled = crate::accounting::AccountingAmount::from_money(settled_gross);
                company_system
                    .pay_dividend(&plan.issuer, &plan.plan_id, &payment_id, day, settled)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                if issuer.listed_stock.as_ref() != Some(&plan.stock) {
                    return Err(SessionCorporateActionsError::Invalid(
                        "分红付款发行人证券关系已改变".into(),
                    ));
                }
            }
        }
        self.settle_dividend_tax(accounts)?;
        Ok(())
    }

    /// 送转事件日结：公告 → R 日按登记快照冻结分配 → R+1 以 NonTradingTransfer
    /// 落账新股并同步投资者持仓、发行股数与 Simple 账面展示事实。
    ///
    /// 事件幂等：重复执行同一日结不重复入账；任何失败向外返回错误，由日终
    /// 候选事务整体回滚，不留下部分状态。
    pub(crate) fn process_stock_distributions_on_day_end(
        &mut self,
        day: crate::calendar::CivilDate,
        calendar: &crate::calendar::TradingCalendar,
        company_system: &mut crate::company::CompanySystem,
        accounts: &mut super::account_book::AccountBook,
    ) -> Result<(), SessionCorporateActionsError> {
        for index in 0..self.stock_distributions.len() {
            let plan = self.stock_distributions[index].plan().clone();
            if self.stock_distributions[index].status()
                == &crate::company::stock_distribution::StockDistributionStatus::Approved
            {
                if day > plan.announced_on {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "送转事件 {} 错过公告日",
                        plan.event_id
                    )));
                }
                if day == plan.announced_on {
                    self.stock_distributions[index]
                        .announce(day)
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                }
            }
            // 已过入账日仍停留在 Registered 的账簿属于错过入账：显式失败而不是
            // 静默跳过（Approved/Announced 的错过公告日、登记日已有对称防护）。
            if self.stock_distributions[index].status()
                == &crate::company::stock_distribution::StockDistributionStatus::Registered
                && day > plan.ex_rights_on
            {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "送转事件 {} 错过入账日",
                    plan.event_id
                )));
            }
            if day == plan.registered_on {
                if self.stock_distributions[index].status()
                    != &crate::company::stock_distribution::StockDistributionStatus::Announced
                {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "送转事件 {} 登记日前未完成公告",
                        plan.event_id
                    )));
                }
                let registry_index = self
                    .registries
                    .iter()
                    .position(|registry| {
                        registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                    })
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("送转登记日缺少匹配股东名册".into())
                    })?;
                let snapshot = self.registries[registry_index]
                    .register(plan.event_id.clone(), day)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .clone();
                self.stock_distributions[index]
                    .register(snapshot, calendar)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                let approved_shares = self.stock_distributions[index]
                    .receipt()
                    .map(|receipt| receipt.approved_total_new_shares)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("送转登记后缺少分配回执".into())
                    })?;
                let finance_fact = company_system
                    .stock_distribution_facts(&plan.issuer)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .into_iter()
                    .find(|fact| fact.event_id == plan.event_id)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("送转登记缺少Simple声明事实".into())
                    })?;
                if finance_fact.new_shares != approved_shares {
                    return Err(SessionCorporateActionsError::Invalid(
                        "送转登记日获批新增股数与Simple声明不一致".into(),
                    ));
                }
            } else if day > plan.registered_on
                && self.stock_distributions[index].registration().is_none()
            {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "送转事件 {} 错过登记日",
                    plan.event_id
                )));
            }
            if day != plan.ex_rights_on
                || self.stock_distributions[index].status()
                    != &crate::company::stock_distribution::StockDistributionStatus::Registered
            {
                continue;
            }
            let receipt = self.stock_distributions[index]
                .receipt()
                .cloned()
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("送转入账缺少冻结分配回执".into())
                })?;
            let credit_lots = holder_credit_lots(&receipt, day)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            let mut changes = Vec::with_capacity(credit_lots.len());
            for lot in &credit_lots {
                changes.push(DayNetChange {
                    holder: lot.holder.clone(),
                    change: i128::try_from(lot.qty).map_err(|_| {
                        SessionCorporateActionsError::Invalid("送转入账股数超出日结范围".into())
                    })?,
                    acquisition: Some(NetAcquisition {
                        lot_id: format!(
                            "stock-distribution:{}:{}",
                            plan.event_id,
                            holder_key(&lot.holder)
                        ),
                        source: AcquisitionSource::CorporateAction {
                            event: plan.event_id.clone(),
                        },
                        restriction: lot.restriction.clone(),
                    }),
                });
            }
            let registry_index = self
                .registries
                .iter()
                .position(|registry| {
                    registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                })
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("送转入账缺少匹配股东名册".into())
                })?;
            self.registries[registry_index]
                .close_day(ShareDayRequest {
                    event_id: format!("stock-distribution:{}", plan.event_id),
                    day,
                    scope: MovementScope::NonTradingTransfer {
                        basis: plan.approval_reference.clone(),
                    },
                    changes,
                })
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            for lot in &credit_lots {
                let HolderId::Account(account) = &lot.holder else {
                    continue;
                };
                let account = *account;
                let account_state = accounts.get_mut(&account).ok_or_else(|| {
                    SessionCorporateActionsError::Invalid(format!(
                        "送转股东账户 {account:?} 不存在"
                    ))
                })?;
                account_state
                    .credit_position_shares(plan.stock.clone(), lot.qty)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
            // 送转新股当日进入税账（R+1 到账日即税法取得日：财税〔2012〕85号
            // 第六条（八）将股份股利与公积金转增股本列为个人取得的股票、
            // 第一条第二款持股期限自取得之日起算、第三条按每日日终净增与
            // 取得日先后先进先出——送转批次不与原股份视为同一批次）。
            // 以非交易过户回执自身事件身份作为同日正向续记落账，与当日
            // 公开市场日结（先落账）分列；限售继承经 convert_tax_class 映射为
            // StatutoryRestricted（85号第四条解禁前 10%、档期自解禁日起算）。
            for lot in &credit_lots {
                let HolderId::Account(account) = &lot.holder else {
                    continue;
                };
                let account = *account;
                let Some(book) = self
                    .dividend_tax_books
                    .iter_mut()
                    .find(|book| book.account() == account && book.stock() == &plan.stock)
                else {
                    // 未配置个人税账的持有人（机构/游资/TreatmentNotConfigured）
                    // 不产生个人税事实，显式跳过而非虚构税账。
                    continue;
                };
                let receipt_event_id = format!("stock-distribution:{}", plan.event_id);
                let acquisition = crate::company::cash_dividend_tax::DividendTaxLot {
                    id: format!(
                        "tax:stock-distribution:{}:{}",
                        plan.event_id,
                        holder_key(&lot.holder)
                    ),
                    qty: lot.qty,
                    acquired_on: day,
                    source: crate::company::cash_dividend_tax::convert_tax_source(
                        &AcquisitionSource::CorporateAction {
                            event: plan.event_id.clone(),
                        },
                    ),
                    class: crate::company::cash_dividend_tax::convert_tax_class(&lot.restriction),
                };
                book.record_net_day(
                    nontrading_tax_day_event_id(&receipt_event_id, account),
                    day,
                    i128::from(lot.qty),
                    Some(acquisition),
                )
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
            company_system
                .record_stock_distribution_credit(
                    &plan.issuer,
                    &plan.event_id,
                    day,
                    receipt.approved_total_new_shares,
                )
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            self.stock_distributions[index]
                .mark_credited(day)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        }
        Ok(())
    }

    /// 拆股／缩股事件日结：公告 → R 日冻结快照换算 → R+1 按 `MovementScope::
    /// ShareReDenomination` 重新计值入账（换算新增 lot 或 FIFO 核减）、账户持仓
    /// 同步、税账同日续记／核减、Simple 账面回填。
    ///
    /// 事件幂等：重复执行同一日结不重复换算或入账；任何失败向外返回错误，由日终
    /// 候选事务整体回滚，不留下部分状态。
    pub(crate) fn process_share_splits_on_day_end(
        &mut self,
        day: crate::calendar::CivilDate,
        calendar: &crate::calendar::TradingCalendar,
        company_system: &mut crate::company::CompanySystem,
        accounts: &mut super::account_book::AccountBook,
    ) -> Result<(), SessionCorporateActionsError> {
        use crate::company::share_split::{holder_inherited_restriction, ShareSplitStatus};
        for index in 0..self.share_splits.len() {
            let plan = self.share_splits[index].plan().clone();
            if self.share_splits[index].status() == &ShareSplitStatus::Approved {
                if day > plan.announced_on {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "拆股／缩股事件 {} 错过公告日",
                        plan.event_id
                    )));
                }
                if day == plan.announced_on {
                    self.share_splits[index]
                        .announce(day)
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                }
            }
            // 已过换算日仍停留在 Registered 的账簿属于错过入账：显式失败。
            if self.share_splits[index].status() == &ShareSplitStatus::Registered
                && day > plan.ex_rights_on
            {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "拆股／缩股事件 {} 错过换算入账日",
                    plan.event_id
                )));
            }
            if day == plan.registered_on {
                if self.share_splits[index].status() != &ShareSplitStatus::Announced {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "拆股／缩股事件 {} 登记日前未完成公告",
                        plan.event_id
                    )));
                }
                let registry_index = self
                    .registries
                    .iter()
                    .position(|registry| {
                        registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                    })
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(
                            "拆股／缩股登记日缺少匹配股东名册".into(),
                        )
                    })?;
                let snapshot = self.registries[registry_index]
                    .register(plan.event_id.clone(), day)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .clone();
                self.share_splits[index]
                    .register(snapshot, calendar)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                company_system
                    .share_split_facts(&plan.issuer)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .into_iter()
                    .find(|fact| fact.event_id == plan.event_id)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(
                            "拆股／缩股登记缺少 Simple 声明事实".into(),
                        )
                    })?;
            } else if day > plan.registered_on
                && self.share_splits[index].registration().is_none()
            {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "拆股／缩股事件 {} 错过登记日",
                    plan.event_id
                )));
            }
            if day != plan.ex_rights_on
                || self.share_splits[index].status() != &ShareSplitStatus::Registered
            {
                continue;
            }
            let receipt = self.share_splits[index]
                .receipt()
                .cloned()
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("拆股／缩股入账缺少冻结换算回执".into())
                })?;
            // 构造重新计值净变动：正向新增 lot（取得日 = R+1、继承限售）或
            // 负向 FIFO 核减（不是处置，取得日由存活 lot 延续）。
            let mut changes = Vec::new();
            let mut account_deltas = Vec::new();
            for outcome in &receipt.holders {
                let original = outcome.original_shares;
                let new_shares = outcome.new_shares;
                if new_shares == original {
                    continue;
                }
                let restriction = if new_shares > original {
                    holder_inherited_restriction(outcome, day).map_err(|error| {
                        SessionCorporateActionsError::Invalid(error.to_string())
                    })?
                } else {
                    ShareRestriction::Unrestricted
                };
                if new_shares > original {
                    changes.push(DayNetChange {
                        holder: outcome.holder.clone(),
                        change: i128::try_from(new_shares - original).map_err(|_| {
                            SessionCorporateActionsError::Invalid(
                                "拆股入账股数超出日结范围".into(),
                            )
                        })?,
                        acquisition: Some(NetAcquisition {
                            lot_id: format!(
                                "share-split:{}:{}",
                                plan.event_id,
                                holder_key(&outcome.holder)
                            ),
                            source: AcquisitionSource::CorporateAction {
                                event: plan.event_id.clone(),
                            },
                            restriction: restriction.clone(),
                        }),
                    });
                } else {
                    changes.push(DayNetChange {
                        holder: outcome.holder.clone(),
                        change: -(i128::try_from(original - new_shares).map_err(|_| {
                            SessionCorporateActionsError::Invalid(
                                "缩股核减股数超出日结范围".into(),
                            )
                        })?),
                        acquisition: None,
                    });
                }
                if let HolderId::Account(account) = &outcome.holder {
                    account_deltas.push((*account, original, new_shares, restriction));
                }
            }
            let registry_index = self
                .registries
                .iter()
                .position(|registry| {
                    registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                })
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("拆股／缩股入账缺少匹配股东名册".into())
                })?;
            self.registries[registry_index]
                .close_day(ShareDayRequest {
                    event_id: format!("share-split:{}", plan.event_id),
                    day,
                    scope: MovementScope::ShareReDenomination {
                        basis: plan.approval_reference.clone(),
                    },
                    changes,
                })
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            for (account, original, new_shares, _) in &account_deltas {
                let account_state = accounts.get_mut(account).ok_or_else(|| {
                    SessionCorporateActionsError::Invalid(format!(
                        "拆股／缩股股东账户 {account:?} 不存在"
                    ))
                })?;
                if new_shares > original {
                    account_state
                        .credit_position_shares(plan.stock.clone(), new_shares - original)
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                } else {
                    account_state
                        .apply_share_redenomination(plan.stock.clone(), *original, *new_shares)
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                }
            }
            // 税账同日续记／核减：拆股增量按「取得日 = R+1」同日正向续记（与送转
            // 同一口径，85号保守解释）；缩股核减走非应税重新计值核减。
            for (account, original, new_shares, restriction) in &account_deltas {
                let Some(book) = self
                    .dividend_tax_books
                    .iter_mut()
                    .find(|book| book.account() == *account && book.stock() == &plan.stock)
                else {
                    continue;
                };
                let receipt_event_id = format!("share-split:{}", plan.event_id);
                let tax_event_id = nontrading_tax_day_event_id(&receipt_event_id, *account);
                if new_shares > original {
                    let acquisition = crate::company::cash_dividend_tax::DividendTaxLot {
                        id: format!(
                            "tax:share-split:{}:{}",
                            plan.event_id,
                            holder_key(&HolderId::Account(*account))
                        ),
                        qty: new_shares - original,
                        acquired_on: day,
                        source: crate::company::cash_dividend_tax::convert_tax_source(
                            &AcquisitionSource::CorporateAction {
                                event: plan.event_id.clone(),
                            },
                        ),
                        class: crate::company::cash_dividend_tax::convert_tax_class(restriction),
                    };
                    book.record_net_day(
                        tax_event_id,
                        day,
                        i128::try_from(new_shares - original)
                            .map_err(|_| SessionCorporateActionsError::Invalid("拆股税账增量溢出".into()))?,
                        Some(acquisition),
                    )
                    .map_err(|error| {
                        SessionCorporateActionsError::Invalid(error.to_string())
                    })?;
                } else {
                    book.record_redenomination_reduction(
                        tax_event_id,
                        day,
                        original - new_shares,
                    )
                    .map_err(|error| {
                        SessionCorporateActionsError::Invalid(error.to_string())
                    })?;
                }
            }
            company_system
                .record_share_split_credit(
                    &plan.issuer,
                    &plan.event_id,
                    day,
                    receipt.issued_shares_before,
                    receipt.issued_shares_after,
                )
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            self.share_splits[index]
                .mark_settled(day)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        }
        Ok(())
    }

    /// 配股／增发事件日结：公告 → R 日权证派发 → 缴款期认购划扣（NPC 默认足额
    /// ＋排队显式认购）→ L 关窗 → L+2 结算（成功划款入账／失败退款）。
    ///
    /// 事件幂等：重复执行同一日结不重复划扣或入账；任何失败向外返回错误，由日终
    /// 候选事务整体回滚，不留下部分状态。
    pub(crate) fn process_rights_offerings_on_day_end(
        &mut self,
        day: crate::calendar::CivilDate,
        calendar: &crate::calendar::TradingCalendar,
        company_system: &mut crate::company::CompanySystem,
        accounts: &mut super::account_book::AccountBook,
    ) -> Result<(), SessionCorporateActionsError> {
        for index in 0..self.rights_offerings.len() {
            let plan = self.rights_offerings[index].plan().clone();
            if self.rights_offerings[index].status()
                == &crate::company::rights_offering::RightsOfferingStatus::Approved
            {
                if day > plan.announced_on {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "配股事件 {} 错过公告日",
                        plan.event_id
                    )));
                }
                if day == plan.announced_on {
                    self.rights_offerings[index]
                        .announce(day)
                        .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                }
            }
            if day == plan.registered_on {
                if self.rights_offerings[index].status()
                    != &crate::company::rights_offering::RightsOfferingStatus::Announced
                {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "配股事件 {} 登记日前未完成公告",
                        plan.event_id
                    )));
                }
                let registry_index = self
                    .registries
                    .iter()
                    .position(|registry| {
                        registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                    })
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("配股登记日缺少匹配股东名册".into())
                    })?;
                let snapshot = self.registries[registry_index]
                    .register(plan.event_id.clone(), day)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .clone();
                self.rights_offerings[index]
                    .entitle(snapshot, calendar)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                company_system
                    .rights_offering_facts(&plan.issuer)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .into_iter()
                    .find(|fact| fact.event_id == plan.event_id)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(
                            "配股登记缺少 Simple 声明事实".into(),
                        )
                    })?;
            } else if day > plan.registered_on
                && self.rights_offerings[index].registration().is_none()
            {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "配股事件 {} 错过登记日",
                    plan.event_id
                )));
            }
            // 缴款期认购处理。
            if self.rights_offerings[index].status()
                == &crate::company::rights_offering::RightsOfferingStatus::Entitled
                && plan.payment_window_contains(day)
            {
                // NPC 默认足额认购（FullByDefault）：缴款起始日日终一次性足额认购，
                // 本人真实现金不足部分放弃并如实记录（2026-10-07 产品决策）。
                if day == plan.payment_start_on {
                    let entitlements: Vec<(AccountId, u64)> = self.rights_offerings[index]
                        .entitlement()
                        .map(|receipt| {
                            receipt
                                .entitlements
                                .iter()
                                .filter_map(|entry| match entry.holder {
                                    HolderId::Account(account)
                                        if account.0 != 0
                                            && accounts.get(&account).is_some_and(|state| {
                                                state.kind() != crate::account::AccountKind::Player
                                            }) =>
                                    {
                                        Some((account, entry.rights_shares))
                                    }
                                    _ => None,
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    for (account, rights_shares) in entitlements {
                        let price = plan.price_per_share;
                        let cash = accounts
                            .get(&account)
                            .map(|state| state.cash())
                            .unwrap_or(crate::money::Money::ZERO);
                        let affordable = u64::try_from(
                            (cash.cents().max(0) as u128) / (price.cents().unsigned_abs() as u128),
                        )
                        .unwrap_or(0);
                        let paid = rights_shares.min(affordable);
                        let paid_amount = crate::money::Money::from_cents(
                            i64::try_from(
                                u128::from(paid) * u128::from(price.cents().unsigned_abs()),
                            )
                            .map_err(|_| {
                                SessionCorporateActionsError::Invalid("配股缴款金额溢出".into())
                            })?,
                        );
                        if paid > 0 {
                            accounts
                                .get_mut(&account)
                                .ok_or_else(|| {
                                    SessionCorporateActionsError::Invalid(format!(
                                        "配股股东账户 {account:?} 不存在"
                                    ))
                                })?
                                .debit_cash(paid_amount)
                                .map_err(|error| {
                                    SessionCorporateActionsError::Invalid(error.to_string())
                                })?;
                        }
                        self.rights_offerings[index]
                            .record_subscription(
                                crate::company::rights_offering::RightsSubscriptionRecord {
                                    holder: HolderId::Account(account),
                                    requested_shares: rights_shares,
                                    price_per_share: price,
                                    submitted_on: day,
                                    origin: crate::company::rights_offering::SubscriptionOrigin::NpcFullByDefault,
                                    paid_shares: paid,
                                    paid_amount,
                                    waived_shares: rights_shares - paid,
                                },
                            )
                            .map_err(|error| {
                                SessionCorporateActionsError::Invalid(error.to_string())
                            })?;
                    }
                }
                // 排队的显式认购（含此前日终失败回滚遗留的陈旧条目）：按入队
                // 顺序处理，现金复核后划扣（不足部分放弃并如实记录）。极端
                // 竞态（如恢复后队列与额度不一致）下，超额度或无法入账的条目
                // 出显式拒绝回执并从队列移除，不令整个日终失败（幂等、留痕）。
                let entitlement_receipt = self.rights_offerings[index]
                    .entitlement()
                    .cloned()
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("配股认购处理缺少权证回执".into())
                    })?;
                let queue: Vec<QueuedRightsSubscription> = self
                    .rights_subscription_queue
                    .iter()
                    .filter(|queued| {
                        queued.event_id == plan.event_id && queued.submitted_on <= day
                    })
                    .cloned()
                    .collect();
                // 公开配售剩余额度 = 公开额度 − 已入账认购（无具名权利持有人）。
                let mut open_remaining = entitlement_receipt.open_subscription_shares;
                for record in self.rights_offerings[index].subscriptions() {
                    if !entitlement_receipt
                        .entitlements
                        .iter()
                        .any(|entry| entry.holder == record.holder)
                    {
                        open_remaining = open_remaining.checked_sub(record.requested_shares).ok_or_else(|| {
                            SessionCorporateActionsError::Invalid(
                                "配股已入账认购超出公开配售额度".into(),
                            )
                        })?;
                    }
                }
                for queued in queue {
                    let holder = HolderId::Account(queued.account);
                    let open_path = !entitlement_receipt
                        .entitlements
                        .iter()
                        .any(|entry| entry.holder == holder);
                    if open_path && queued.requested_shares > open_remaining {
                        self.rejected_rights_subscriptions
                            .push(RejectedRightsSubscription {
                                event_id: queued.event_id.clone(),
                                account: queued.account,
                                requested_shares: queued.requested_shares,
                                submitted_on: queued.submitted_on,
                                rejected_on: day,
                                reason: format!(
                                    "公开配售剩余额度 {open_remaining} 股，申请 {} 股超出额度，按队列序显式拒绝",
                                    queued.requested_shares
                                ),
                            });
                        self.rights_subscription_queue.retain(|existing| {
                            !(existing.event_id == queued.event_id
                                && existing.account == queued.account)
                        });
                        continue;
                    }
                    let price = plan.price_per_share;
                    let cash = accounts
                        .get(&queued.account)
                        .map(|state| state.cash())
                        .unwrap_or(crate::money::Money::ZERO);
                    let affordable = u64::try_from(
                        (cash.cents().max(0) as u128) / (price.cents().unsigned_abs() as u128),
                    )
                    .unwrap_or(0);
                    let paid = queued.requested_shares.min(affordable);
                    let paid_amount = crate::money::Money::from_cents(
                        i64::try_from(
                            u128::from(paid) * u128::from(price.cents().unsigned_abs()),
                        )
                        .map_err(|_| {
                            SessionCorporateActionsError::Invalid("配股缴款金额溢出".into())
                        })?,
                    );
                    // 先入账后划扣：入账被拒时未发生任何现金变动，显式拒绝
                    // 留痕并移除，不令整个日终失败。
                    let record = crate::company::rights_offering::RightsSubscriptionRecord {
                        holder,
                        requested_shares: queued.requested_shares,
                        price_per_share: price,
                        submitted_on: queued.submitted_on,
                        origin: crate::company::rights_offering::SubscriptionOrigin::Explicit,
                        paid_shares: paid,
                        paid_amount,
                        waived_shares: queued.requested_shares - paid,
                    };
                    if let Err(error) = self.rights_offerings[index].record_subscription(record) {
                        self.rejected_rights_subscriptions
                            .push(RejectedRightsSubscription {
                                event_id: queued.event_id.clone(),
                                account: queued.account,
                                requested_shares: queued.requested_shares,
                                submitted_on: queued.submitted_on,
                                rejected_on: day,
                                reason: format!("配股认购入账校验失败：{error}"),
                            });
                        self.rights_subscription_queue.retain(|existing| {
                            !(existing.event_id == queued.event_id
                                && existing.account == queued.account)
                        });
                        continue;
                    }
                    if paid > 0 {
                        accounts
                            .get_mut(&queued.account)
                            .ok_or_else(|| {
                                SessionCorporateActionsError::Invalid(format!(
                                    "配股认购账户 {:?} 不存在",
                                    queued.account
                                ))
                            })?
                            .debit_cash(paid_amount)
                            .map_err(|error| {
                                SessionCorporateActionsError::Invalid(error.to_string())
                            })?;
                    }
                    if open_path {
                        open_remaining -= queued.requested_shares;
                    }
                    self.rights_subscription_queue.retain(|existing| {
                        !(existing.event_id == queued.event_id
                            && existing.account == queued.account)
                    });
                }
            }
            if day == plan.payment_deadline_on {
                self.rights_offerings[index]
                    .close_payment_window(day)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            } else if day > plan.payment_deadline_on
                && self.rights_offerings[index].status()
                    == &crate::company::rights_offering::RightsOfferingStatus::Entitled
            {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "配股事件 {} 错过缴款截止关窗",
                    plan.event_id
                )));
            }
            if day != plan.settlement_on
                || self.rights_offerings[index].status()
                    != &crate::company::rights_offering::RightsOfferingStatus::Closed
            {
                if day > plan.settlement_on
                    && self.rights_offerings[index].status()
                        == &crate::company::rights_offering::RightsOfferingStatus::Closed
                {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "配股事件 {} 错过结算日",
                        plan.event_id
                    )));
                }
                continue;
            }
            let failed = self.rights_offerings[index]
                .determine_failure()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            let subscriptions: Vec<crate::company::rights_offering::RightsSubscriptionRecord> =
                self.rights_offerings[index].subscriptions().to_vec();
            let mut holder_rows = Vec::with_capacity(subscriptions.len());
            let mut total_paid_shares = 0_u64;
            let mut total_paid_amount = crate::money::Money::ZERO;
            let mut refunded_total = crate::money::Money::ZERO;
            for subscription in &subscriptions {
                let refunded = if failed {
                    subscription.paid_amount
                } else {
                    crate::money::Money::ZERO
                };
                if failed && refunded.cents() > 0 {
                    if let HolderId::Account(account) = &subscription.holder {
                        accounts
                            .get_mut(account)
                            .ok_or_else(|| {
                                SessionCorporateActionsError::Invalid(format!(
                                    "配股退款账户 {account:?} 不存在"
                                ))
                            })?
                            .credit_cash(refunded)
                            .map_err(|error| {
                                SessionCorporateActionsError::Invalid(error.to_string())
                            })?;
                    }
                }
                total_paid_shares = total_paid_shares
                    .checked_add(subscription.paid_shares)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("配股认购合计溢出".into())
                    })?;
                total_paid_amount = total_paid_amount
                    .add(subscription.paid_amount)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                refunded_total = refunded_total
                    .add(refunded)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                holder_rows.push(crate::company::rights_offering::HolderRightsSettlement {
                    holder: subscription.holder.clone(),
                    paid_shares: subscription.paid_shares,
                    paid_amount: subscription.paid_amount,
                    waived_shares: subscription.waived_shares,
                    refunded_amount: refunded,
                });
            }
            if !failed && total_paid_shares > 0 {
                // 新股入账：NonTradingTransfer 落 lot（锁定期继承定向对象），投资者
                // 账户真实加股；配股新股按取得日（上市日）进个人税账 FIFO。
                let receipt_event_id = format!("rights-offering:{}", plan.event_id);
                let mut changes = Vec::with_capacity(subscriptions.len());
                for subscription in &subscriptions {
                    if subscription.paid_shares == 0 {
                        continue;
                    }
                    let lock_until = self.rights_offerings[index]
                        .entitlement()
                        .and_then(|receipt| {
                            receipt
                                .entitlements
                                .iter()
                                .find(|entry| entry.holder == subscription.holder)
                                .and_then(|entry| entry.lock_until)
                        })
                        .or({
                            // 面向全体股东模式恒为 None；防御式兜底。
                            None::<crate::calendar::CivilDate>
                        });
                    let restriction = match lock_until {
                        Some(release_on) => ShareRestriction::Restricted {
                            reason: "directed-placement-lock".into(),
                            release_on,
                        },
                        None => ShareRestriction::Unrestricted,
                    };
                    changes.push(DayNetChange {
                        holder: subscription.holder.clone(),
                        change: i128::try_from(subscription.paid_shares).map_err(|_| {
                            SessionCorporateActionsError::Invalid("配股入账股数超出日结范围".into())
                        })?,
                        acquisition: Some(NetAcquisition {
                            lot_id: format!(
                                "rights-offering:{}:{}",
                                plan.event_id,
                                holder_key(&subscription.holder)
                            ),
                            source: AcquisitionSource::CorporateAction {
                                event: plan.event_id.clone(),
                            },
                            restriction: restriction.clone(),
                        }),
                    });
                }
                if !changes.is_empty() {
                    let registry_index = self
                        .registries
                        .iter()
                        .position(|registry| {
                            registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                        })
                        .ok_or_else(|| {
                            SessionCorporateActionsError::Invalid("配股入账缺少匹配股东名册".into())
                        })?;
                    self.registries[registry_index]
                        .close_day(ShareDayRequest {
                            event_id: receipt_event_id.clone(),
                            day,
                            scope: MovementScope::NonTradingTransfer {
                                basis: plan.approval_reference.clone(),
                            },
                            changes,
                        })
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                    for subscription in &subscriptions {
                        if subscription.paid_shares == 0 {
                            continue;
                        }
                        if let HolderId::Account(account) = &subscription.holder {
                            let account = *account;
                            accounts
                                .get_mut(&account)
                                .ok_or_else(|| {
                                    SessionCorporateActionsError::Invalid(format!(
                                        "配股股东账户 {account:?} 不存在"
                                    ))
                                })?
                                .credit_position_shares(
                                    plan.stock.clone(),
                                    subscription.paid_shares,
                                )
                                .map_err(|error| {
                                    SessionCorporateActionsError::Invalid(error.to_string())
                                })?;
                            let acquisition_lot = crate::company::cash_dividend_tax::DividendTaxLot {
                                id: format!(
                                    "tax:rights-offering:{}:{}",
                                    plan.event_id, account.0
                                ),
                                qty: subscription.paid_shares,
                                acquired_on: day,
                                source: crate::company::cash_dividend_tax::convert_tax_source(
                                    &AcquisitionSource::CorporateAction {
                                        event: plan.event_id.clone(),
                                    },
                                ),
                                class: crate::company::cash_dividend_tax::convert_tax_class(
                                    &self.rights_offerings[index]
                                        .entitlement()
                                        .and_then(|receipt| {
                                            receipt.entitlements.iter().find(|entry| {
                                                entry.holder == subscription.holder
                                            })
                                        })
                                        .map(|entry| match entry.lock_until {
                                            Some(release_on) => ShareRestriction::Restricted {
                                                reason: "directed-placement-lock".into(),
                                                release_on,
                                            },
                                            None => ShareRestriction::Unrestricted,
                                        })
                                        .unwrap_or(ShareRestriction::Unrestricted),
                                ),
                            };
                            if let Some(book) = self
                                .dividend_tax_books
                                .iter_mut()
                                .find(|book| book.account() == account
                                    && book.stock() == &plan.stock)
                            {
                                book.record_net_day(
                                    nontrading_tax_day_event_id(&receipt_event_id, account),
                                    day,
                                    i128::from(subscription.paid_shares),
                                    Some(acquisition_lot),
                                )
                                .map_err(|error| {
                                    SessionCorporateActionsError::Invalid(error.to_string())
                                })?;
                            }
                        }
                    }
                }
            }
            // 失败路径：资金已全额退回认购人，无新股、无募集资金——账面按零发行
            // 登记（保持审计轨迹）；成功路径按实际认购登记。
            let finance_shares = if failed { 0 } else { total_paid_shares };
            let proceeds = crate::accounting::AccountingAmount::from_money(if failed {
                crate::money::Money::ZERO
            } else {
                total_paid_amount
            });
            company_system
                .record_rights_offering_settlement(
                    &plan.issuer,
                    &plan.event_id,
                    day,
                    finance_shares,
                    proceeds,
                )
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            self.rights_offerings[index]
                .settle(
                    day,
                    crate::company::rights_offering::RightsSettlementReceipt {
                        event_id: plan.event_id.clone(),
                        settlement_on: day,
                        failed,
                        total_paid_shares,
                        total_paid_amount,
                        refunded_total,
                        holders: holder_rows,
                    },
                )
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if !failed {
                self.rights_offerings[index]
                    .mark_credited(day)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
        }
        Ok(())
    }

    /// 发行人回购日结：公告 → 真实成交汇总（session 从回执聚合传入）→
    /// 窗口截止或额度／数量用尽后的完成与未用合成资金回收。
    pub(crate) fn process_issuer_repurchases_on_day_end(
        &mut self,
        day: crate::calendar::CivilDate,
        issuer_fills: &[crate::company::issuer_repurchase::RepurchaseFillRecord],
        accounts: &mut super::account_book::AccountBook,
        repurchase_account: Option<AccountId>,
        lot_size: u32,
        mut repurchase_finance: Option<&mut crate::company::CompanySystem>,
    ) -> Result<(), SessionCorporateActionsError> {
        for index in 0..self.issuer_repurchases.len() {
            let plan = self.issuer_repurchases[index].plan().clone();
            if self.issuer_repurchases[index].status()
                == &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Approved
            {
                if day > plan.announced_on {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "回购方案 {} 错过公告日",
                        plan.event_id
                    )));
                }
                if day == plan.announced_on {
                    self.issuer_repurchases[index]
                        .announce(day)
                        .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                }
            }
            let in_execution = matches!(
                self.issuer_repurchases[index].status(),
                crate::company::issuer_repurchase::IssuerRepurchaseStatus::Announced
                    | crate::company::issuer_repurchase::IssuerRepurchaseStatus::Executing
            );
            if in_execution && plan.window_contains(day) {
                for fill in issuer_fills
                    .iter()
                    .filter(|fill| fill.day == day && fill.stock == plan.stock)
                {
                    self.issuer_repurchases[index]
                        .record_fill(fill.clone())
                        .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                }
            }
            if !in_execution
                || (day <= plan.window_deadline_on && {
                    // 额度或数量用尽 → 提前完成（显式穷尽口径）。
                    let remaining = self.issuer_repurchases[index]
                        .remaining_budget()
                        .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                    let cheapest_lot = plan
                        .price_cap_per_share
                        .cents()
                        .unsigned_abs()
                        .saturating_mul(u64::from(lot_size).max(1));
                    let share_room = plan
                        .max_shares
                        .saturating_sub(self.issuer_repurchases[index].total_filled_shares());
                    remaining.cents().unsigned_abs() < cheapest_lot || share_room == 0
                })
                || day > plan.window_deadline_on
            {
                if !in_execution {
                    continue;
                }
                // 完成计划：回收未用合成资金（回购账户现金清零，差额不落地）。
                let spent = self.issuer_repurchases[index]
                    .total_spent()
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                let remainder = plan
                    .total_budget
                    .sub(spent)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                if let Some(account) = repurchase_account {
                    let account_cash = accounts
                        .get(&account)
                        .map(|state| state.cash())
                        .unwrap_or(crate::money::Money::ZERO);
                    if account_cash != remainder {
                        return Err(SessionCorporateActionsError::Invalid(format!(
                            "回购账户现金 {account_cash:?} 与计划未用额度 {remainder:?} 不一致"
                        )));
                    }
                    if remainder.cents() > 0 {
                        accounts
                            .get_mut(&account)
                            .ok_or_else(|| {
                                SessionCorporateActionsError::Invalid(format!(
                                    "回购账户 {account:?} 不存在"
                                ))
                            })?
                            .debit_cash(remainder)
                            .map_err(|error| {
                                SessionCorporateActionsError::Invalid(error.to_string())
                            })?;
                    }
                }
                let issuer = self.issuer_repurchases[index].plan().issuer.clone();
                let event_id = self.issuer_repurchases[index].plan().event_id.clone();
                self.issuer_repurchases[index]
                    .complete(day, remainder)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                let spent_amount = crate::accounting::AccountingAmount::from_money(spent);
                let withdrawn_amount = crate::accounting::AccountingAmount::from_money(remainder);
                repurchase_finance
                    .as_mut()
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("回购日终缺少公司系统句柄".into())
                    })?
                    .record_issuer_repurchase_completion(
                        &issuer,
                        &event_id,
                        day,
                        spent_amount,
                        withdrawn_amount,
                    )
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
        }
        Ok(())
    }

    pub(crate) fn close_registries_through(
        &mut self,
        through: crate::calendar::CivilDate,
        accounts: &BTreeMap<AccountId, BTreeMap<StockCode, u64>>,
        confirmations: &BTreeMap<
            AccountId,
            crate::experience::AppendOnlyHistory<super::PersonalTradeConfirmation>,
        >,
        issuer_repurchase_account: Option<AccountId>,
    ) -> Result<(), SessionCorporateActionsError> {
        for registry in &mut self.registries {
            if registry.settled_on() > through {
                return Err(SessionCorporateActionsError::Invalid(
                    "股东名册日期晚于日终日期".into(),
                ));
            }
            let mut day = registry.settled_on();
            while day < through {
                day = day
                    .next()
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                let mut net_by_account = BTreeMap::<AccountId, (i128, Vec<u64>)>::new();
                for (account, rows) in confirmations {
                    for row in rows
                        .iter()
                        .filter(|row| row.civil_date == day && row.code == *registry.stock())
                    {
                        let entry = net_by_account.entry(*account).or_default();
                        let quantity = i128::from(row.quantity_shares);
                        entry.0 = if row.side == crate::orderbook::Side::Buy {
                            entry.0.checked_add(quantity)
                        } else {
                            entry.0.checked_sub(quantity)
                        }
                        .ok_or_else(|| {
                            SessionCorporateActionsError::Invalid("当日成交净股数溢出".into())
                        })?;
                        entry.1.push(row.receipt_id);
                    }
                }
                let mut changes = Vec::new();
                for holding in registry.holdings() {
                    let HolderId::Account(account) = &holding.holder else {
                        // IssuerTreasury（回购专户）：吸收发行人回购账户当日公开市场
                        // 净买入（回购买入即过户专户，63 号第 13 条）。
                        let treasury_net = issuer_repurchase_account
                            .and_then(|account| net_by_account.remove(&account))
                            .map(|(change, receipt_ids)| {
                                let source = receipt_ids
                                    .iter()
                                    .map(u64::to_string)
                                    .collect::<Vec<_>>()
                                    .join(",");
                                (change, source)
                            });
                        match treasury_net {
                            Some((change, source)) if change > 0 => {
                                changes.push(DayNetChange {
                                    holder: holding.holder.clone(),
                                    change,
                                    acquisition: Some(NetAcquisition {
                                        lot_id: format!(
                                            "market:{}:issuer-treasury:{}",
                                            registry.stock().0,
                                            day
                                        ),
                                        source: AcquisitionSource::SecondaryMarket { settlement: source },
                                        restriction: ShareRestriction::Unrestricted,
                                    }),
                                });
                            }
                            _ => {
                                changes.push(DayNetChange {
                                    holder: holding.holder.clone(),
                                    change: 0,
                                    acquisition: None,
                                });
                            }
                        }
                        continue;
                    };
                    let account = *account;
                    let (change, receipt_ids) = net_by_account.remove(&account).unwrap_or_default();
                    let acquisition = if change > 0 {
                        let source = receipt_ids
                            .iter()
                            .map(u64::to_string)
                            .collect::<Vec<_>>()
                            .join(",");
                        Some(NetAcquisition {
                            lot_id: format!("market:{}:{}:{}", registry.stock().0, account.0, day),
                            source: AcquisitionSource::SecondaryMarket { settlement: source },
                            restriction: ShareRestriction::Unrestricted,
                        })
                    } else {
                        None
                    };
                    changes.push(DayNetChange {
                        holder: HolderId::Account(account),
                        change,
                        acquisition,
                    });
                }
                for (account, (change, receipt_ids)) in net_by_account {
                    if Some(account) == issuer_repurchase_account {
                        // 回购账户的净变动必须落入 IssuerTreasury 持有人；名册缺少
                        // Treasury 持有人而回购账户有成交属于配置错误，显式失败。
                        return Err(SessionCorporateActionsError::Invalid(
                            "股东名册缺少 IssuerTreasury 持有人承接回购账户成交".into(),
                        ));
                    }
                    if change <= 0 || !accounts.contains_key(&account) {
                        return Err(SessionCorporateActionsError::Invalid(
                            "无既有名册的证券成交账户只能通过真实净买入进入股东登记".into(),
                        ));
                    }
                    let source = receipt_ids
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join(",");
                    changes.push(DayNetChange {
                        holder: HolderId::Account(account),
                        change,
                        acquisition: Some(NetAcquisition {
                            lot_id: format!("market:{}:{}:{}", registry.stock().0, account.0, day),
                            source: AcquisitionSource::SecondaryMarket { settlement: source },
                            restriction: ShareRestriction::Unrestricted,
                        }),
                    });
                }
                registry
                    .close_day(ShareDayRequest {
                        event_id: format!("session-market:{}:{}", registry.stock().0, day),
                        day,
                        scope: MovementScope::PublicMarket,
                        changes,
                    })
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
            for holding in registry.holdings() {
                match &holding.holder {
                    HolderId::Account(account) => {
                        let account = *account;
                        let expected = holding
                            .lots
                            .iter()
                            .try_fold(0_u64, |sum, lot| sum.checked_add(lot.qty))
                            .ok_or_else(|| {
                                SessionCorporateActionsError::Invalid("名册股份溢出".into())
                            })?;
                        let actual = accounts
                            .get(&account)
                            .and_then(|positions| positions.get(registry.stock()))
                            .copied()
                            .unwrap_or(0);
                        if expected != actual {
                            return Err(SessionCorporateActionsError::Invalid(format!(
                                "日终账户 {account:?} 实际持仓 {actual} 与股东名册 {expected} 不一致"
                            )));
                        }
                    }
                    HolderId::IssuerTreasury
                        if issuer_repurchase_account.is_some_and(|account| accounts
                            .get(&account)
                            .is_some_and(|positions| positions.contains_key(registry.stock()))) =>
                    {
                        // 回购专户：名册 Treasury 中由回购买入过户形成的批次
                        // （lot id 前缀 market:{stock}:issuer-treasury:）必须与回购账户
                        // 持仓一致——初始 Treasury 库存与回购新增分账；注销同额核减两侧。
                        let prefix = format!("market:{}:issuer-treasury:", registry.stock().0);
                        let expected = holding
                            .lots
                            .iter()
                            .filter(|lot| lot.id.starts_with(&prefix))
                            .try_fold(0_u64, |sum, lot| sum.checked_add(lot.qty))
                            .ok_or_else(|| {
                                SessionCorporateActionsError::Invalid("名册股份溢出".into())
                            })?;
                        let account = issuer_repurchase_account.expect("checked above");
                        let actual = accounts
                            .get(&account)
                            .and_then(|positions| positions.get(registry.stock()))
                            .copied()
                            .unwrap_or(0);
                        if expected != actual {
                            return Err(SessionCorporateActionsError::Invalid(format!(
                                "回购专户名册回购批次 {expected} 与回购账户持仓 {actual} 不一致"
                            )));
                        }
                    }
                    _ => {}
                }
            }
        }
        self.sync_dividend_tax_days()?;
        Ok(())
    }

    fn sync_dividend_tax_days(&mut self) -> Result<(), SessionCorporateActionsError> {
        let receipts: Vec<(StockCode, crate::company::share_registry::ShareDayReceipt)> = self
            .registries
            .iter()
            .flat_map(|registry| {
                let stock = registry.stock().clone();
                registry
                    .receipts()
                    .iter()
                    .map(move |receipt| (stock.clone(), receipt.clone()))
            })
            .collect();
        let tax_book_keys: Vec<_> = self
            .dividend_tax_books
            .iter()
            .map(|book| (book.account(), book.stock().clone()))
            .collect();
        for (account, stock) in tax_book_keys {
            for (receipt_stock, receipt) in receipts.iter().filter(|(known, _)| known == &stock) {
                // 税账事件 id 按 scope 分列：公开市场日结沿用「证券+账户+自然日」，
                // 非交易过户（送转到账）以回执自身事件身份派生。穷尽 match 使
                // 未来新增 MovementScope 变体在此编译期强制显式决策，不允许
                // 未知 scope 静默跳过（铁律 2）。
                let event_id = match &receipt.request.scope {
                    MovementScope::PublicMarket => format!(
                        "session-market:{}:{}:{}",
                        receipt_stock.0, account.0, receipt.request.day
                    ),
                    MovementScope::NonTradingTransfer { .. }
                    | MovementScope::IssuerRepurchaseCancellation { .. }
                    | MovementScope::ShareReDenomination { .. } => {
                        // 回购注销只核减 IssuerTreasury，无 Account 持有人分录；
                        // 此处事件 id 派生与送转共用，实际不会命中任何税账。
                        nontrading_tax_day_event_id(&receipt.request.event_id, account)
                    }
                };
                let existing_day = self
                    .dividend_tax_books
                    .iter()
                    .find(|book| book.account() == account && book.stock() == receipt_stock)
                    .map(|book| {
                        book.receipt_by_event(&event_id).is_some()
                            || book.redenomination_by_event(&event_id).is_some()
                    })
                    .unwrap_or(false);
                if existing_day {
                    continue;
                }
                let Some(change) = receipt
                    .request
                    .changes
                    .iter()
                    .find(|change| change.holder == HolderId::Account(account))
                else {
                    continue;
                };
                let acquisition = change.acquisition.as_ref().map(|acquisition| {
                    crate::company::cash_dividend_tax::DividendTaxLot {
                        id: format!("tax:{}", acquisition.lot_id),
                        qty: change
                            .change
                            .unsigned_abs()
                            .try_into()
                            .expect("positive change fits u64"),
                        acquired_on: receipt.request.day,
                        source: crate::company::cash_dividend_tax::convert_tax_source(
                            &acquisition.source,
                        ),
                        class: crate::company::cash_dividend_tax::convert_tax_class(
                            &acquisition.restriction,
                        ),
                    }
                });
                let book = self
                    .dividend_tax_books
                    .iter_mut()
                    .find(|book| book.account() == account && book.stock() == receipt_stock)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(format!(
                            "账户 {account:?} 缺少 {receipt_stock:?} 的股息税账"
                        ))
                    })?;
                if matches!(
                    receipt.request.scope,
                    MovementScope::ShareReDenomination { .. }
                ) && change.change < 0
                {
                    // 缩股核减：FIFO 减少税基 lot，但不产生应税处置（重新计值
                    // 不是转让）；取得日由存活 lot 延续。
                    let removed = u64::try_from(change.change.unsigned_abs()).map_err(|_| {
                        SessionCorporateActionsError::Invalid("缩股核减数量超出 u64".into())
                    })?;
                    book.record_redenomination_reduction(
                        event_id,
                        receipt.request.day,
                        removed,
                    )
                    .map_err(|error| {
                        SessionCorporateActionsError::Invalid(error.to_string())
                    })?;
                } else {
                    book.record_net_day(event_id, receipt.request.day, change.change, acquisition)
                        .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn validate(
        &self,
        accounts: &BTreeMap<AccountId, BTreeMap<StockCode, u64>>,
        company_system: &crate::company::CompanySystem,
        current_date: crate::calendar::CivilDate,
        issuer_repurchase_account_id: Option<AccountId>,
    ) -> Result<(), SessionCorporateActionsError> {
        let issuers = company_system.issuers();
        for tax_book in &self.dividend_tax_books {
            tax_book
                .validate()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if !accounts.contains_key(&tax_book.account()) {
                return Err(SessionCorporateActionsError::Invalid(
                    "股息税账引用了不存在的账户".into(),
                ));
            }
            if !self.registries.iter().any(|registry| {
                registry.stock() == tax_book.stock()
                    && registry
                        .holdings()
                        .iter()
                        .any(|holding| holding.holder == HolderId::Account(tax_book.account()))
            }) {
                return Err(SessionCorporateActionsError::Invalid(
                    "股息税账缺少对应的股东登记持有人".into(),
                ));
            }
        }
        // 送转×税账勾稽：每个已配置税账的名册回执（公开市场与非交易过户，
        // 穷尽 match，未来 scope 变体编译期强制显式决策）都必须已按对应
        // 事件 id 入账。缺失说明同步被静默跳过或存档被篡改，恢复时显式
        // 失败而不是让税基悄悄缺股（集成修复轮 major 的防御面）。
        for tax_book in &self.dividend_tax_books {
            let account = tax_book.account();
            let Some(registry) = self
                .registries
                .iter()
                .find(|registry| registry.stock() == tax_book.stock())
            else {
                // 名册存在性已由上方校验保证，此处不可达；防御式显式失败。
                return Err(SessionCorporateActionsError::Invalid(
                    "股息税账缺少对应的股东名册".into(),
                ));
            };
            for receipt in registry.receipts() {
                if !receipt
                    .request
                    .changes
                    .iter()
                    .any(|change| change.holder == HolderId::Account(account))
                {
                    continue;
                }
                let expected_event_id = match &receipt.request.scope {
                    MovementScope::PublicMarket => format!(
                        "session-market:{}:{}:{}",
                        registry.stock().0,
                        account.0,
                        receipt.request.day
                    ),
                    MovementScope::NonTradingTransfer { .. } => {
                        nontrading_tax_day_event_id(&receipt.request.event_id, account)
                    }
                    // 拆股正向增量走日结续记；缩股负向核减走重新计值回执。
                    MovementScope::ShareReDenomination { .. } => {
                        nontrading_tax_day_event_id(&receipt.request.event_id, account)
                    }
                    // 回购注销只核减 IssuerTreasury；该回执不含任何 Account 持有人
                    // 变动，不会进入任何税账的期望集（上面的 continue 已过滤）。
                    MovementScope::IssuerRepurchaseCancellation { .. } => continue,
                };
                let has_tax_fact = match &receipt.request.scope {
                    MovementScope::ShareReDenomination { .. } => {
                        tax_book.receipt_by_event(&expected_event_id).is_some()
                            || tax_book
                                .redenomination_by_event(&expected_event_id)
                                .is_some()
                    }
                    _ => tax_book.receipt_by_event(&expected_event_id).is_some(),
                };
                if !has_tax_fact {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "账户 {account:?} 的股息税账缺少名册回执 {} 的日结事实：\
                         送转×税账交互未入账，拒绝静默缺股",
                        receipt.request.event_id
                    )));
                }
            }
        }
        let mut seen = BTreeSet::new();
        for registry in &self.registries {
            if !seen.insert(registry.stock().clone()) {
                return Err(SessionCorporateActionsError::Invalid(
                    "同一证券存在多个股东名册".into(),
                ));
            }
            Self::validate_registry(
                registry,
                accounts,
                issuers,
                issuer_repurchase_account_id,
            )?;
        }
        for book in &self.dividends {
            book.validate()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if book.plan().approved_on > current_date {
                return Err(SessionCorporateActionsError::Invalid(
                    "现金分红批准日期晚于当前会话日期".into(),
                ));
            }
            if !self.registries.iter().any(|registry| {
                registry.stock() == &book.plan().stock && registry.issuer() == &book.plan().issuer
            }) {
                return Err(SessionCorporateActionsError::Invalid(
                    "分红账簿缺少匹配的完整股东名册".into(),
                ));
            }
            let finance_plan = company_system
                .dividend_plan_facts(&book.plan().issuer)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                .into_iter()
                .find(|plan| plan.plan_id == book.plan().plan_id)
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("分红账簿缺少Simple批准事实".into())
                })?;
            if book.plan().approved_on != finance_plan.approved_on {
                return Err(SessionCorporateActionsError::Invalid(
                    "分红账簿批准日期与Simple事实不一致".into(),
                ));
            }
            if book.registration().is_none() {
                let registry = self
                    .registries
                    .iter()
                    .find(|registry| registry.stock() == &book.plan().stock)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("分红计划缺少名册".into())
                    })?;
                let eligible = registry
                    .holdings()
                    .iter()
                    .filter(|holding| holding.holder != HolderId::IssuerTreasury)
                    .flat_map(|holding| holding.lots.iter())
                    .try_fold(0_u64, |sum, lot| sum.checked_add(lot.qty))
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("分红股份数量溢出".into())
                    })?;
                let gross_cents = i128::from(book.plan().gross_per_share.cents())
                    .checked_mul(i128::from(eligible))
                    .and_then(|cents| i64::try_from(cents).ok())
                    .ok_or_else(|| SessionCorporateActionsError::Invalid("分红总额溢出".into()))?;
                let gross = crate::accounting::AccountingAmount::from_money(
                    crate::money::Money::from_cents(gross_cents),
                );
                if gross != finance_plan.total_gross {
                    return Err(SessionCorporateActionsError::Invalid(
                        "未登记分红计划的名册总股数与Simple批准金额不一致".into(),
                    ));
                }
            }
        }
        company_system
            .validate_cash_dividend_books(&self.dividends)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        for book in &self.stock_distributions {
            book.validate()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if book.plan().approved_on > current_date {
                return Err(SessionCorporateActionsError::Invalid(
                    "送转批准日期晚于当前会话日期".into(),
                ));
            }
            // 恢复校验对称防护：已过入账日的 Registered 账簿属于错过入账，显式失败。
            if book.status()
                == &crate::company::stock_distribution::StockDistributionStatus::Registered
                && current_date > book.plan().ex_rights_on
            {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "送转事件 {} 错过入账日",
                    book.plan().event_id
                )));
            }
            if !self.registries.iter().any(|registry| {
                registry.stock() == &book.plan().stock && registry.issuer() == &book.plan().issuer
            }) {
                return Err(SessionCorporateActionsError::Invalid(
                    "送转账簿缺少匹配的完整股东名册".into(),
                ));
            }
            if let Some(credited_on) = book.credited_on() {
                let registry = self
                    .registries
                    .iter()
                    .find(|registry| registry.stock() == &book.plan().stock)
                    .expect("registry presence was checked above");
                let expected_event = format!("stock-distribution:{}", book.plan().event_id);
                let Some(receipt) = registry.receipt_by_event(&expected_event) else {
                    return Err(SessionCorporateActionsError::Invalid(
                        "送转入账事实与股东名册非交易过户回执不一致".into(),
                    ));
                };
                let credited_total = sum_nontrading_changes(&receipt.request.changes)?;
                if receipt.request.day != credited_on
                    || !matches!(
                        receipt.request.scope,
                        MovementScope::NonTradingTransfer { .. }
                    )
                    || credited_total != book.plan().approved_total_new_shares
                {
                    return Err(SessionCorporateActionsError::Invalid(
                        "送转入账事实与股东名册非交易过户回执不一致".into(),
                    ));
                }
            }
        }
        // 反向勾稽：名册中每条 NonTradingTransfer 回执必须映射到 Credited 状态的
        // 送转或配股账簿，且入账日与数量一致；篡改注入无对应账簿的回执必须被拒。
        let mut credited_books = BTreeMap::new();
        for book in &self.stock_distributions {
            if let Some(credited_on) = book.credited_on() {
                credited_books.insert(
                    format!("stock-distribution:{}", book.plan().event_id),
                    (credited_on, book.plan().approved_total_new_shares),
                );
            }
        }
        for book in &self.rights_offerings {
            if let Some(credited_on) = book.credited_on() {
                credited_books.insert(
                    format!("rights-offering:{}", book.plan().event_id),
                    (
                        credited_on,
                        book.settlement()
                            .map(|settlement| settlement.total_paid_shares)
                            .unwrap_or(0),
                    ),
                );
            }
        }
        for registry in &self.registries {
            for receipt in registry.receipts() {
                if !matches!(
                    receipt.request.scope,
                    MovementScope::NonTradingTransfer { .. }
                ) {
                    continue;
                }
                let Some(&(credited_on, total_shares)) =
                    credited_books.get(&receipt.request.event_id)
                else {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "名册存在无对应已入账送转账簿的非交易过户回执 {}",
                        receipt.request.event_id
                    )));
                };
                let receipt_total = sum_nontrading_changes(&receipt.request.changes)?;
                if receipt.request.day != credited_on || receipt_total != total_shares {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "非交易过户回执 {} 的日期或数量与送转账簿不一致",
                        receipt.request.event_id
                    )));
                }
            }
        }
        // 拆股／缩股账簿校验 + 与名册重新计值回执的双向勾稽。
        let mut settled_split_books = BTreeMap::new();
        for book in &self.share_splits {
            book.validate()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if book.plan().approved_on > current_date {
                return Err(SessionCorporateActionsError::Invalid(
                    "拆股／缩股批准日期晚于当前会话日期".into(),
                ));
            }
            // 恢复校验对称防护：已过换算日的 Registered 账簿属于错过入账。
            if book.status()
                == &crate::company::share_split::ShareSplitStatus::Registered
                && current_date > book.plan().ex_rights_on
            {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "拆股／缩股事件 {} 错过换算入账日",
                    book.plan().event_id
                )));
            }
            if !self.registries.iter().any(|registry| {
                registry.stock() == &book.plan().stock && registry.issuer() == &book.plan().issuer
            }) {
                return Err(SessionCorporateActionsError::Invalid(
                    "拆股／缩股账簿缺少匹配的完整股东名册".into(),
                ));
            }
            if let Some(settled_on) = book.settled_on() {
                let Some(receipt) = book.receipt() else {
                    return Err(SessionCorporateActionsError::Invalid(
                        "已入账拆股／缩股账簿缺少冻结换算回执".into(),
                    ));
                };
                settled_split_books.insert(
                    format!("share-split:{}", book.plan().event_id),
                    (
                        settled_on,
                        receipt.issued_shares_after as i128
                            - receipt.issued_shares_before as i128,
                    ),
                );
            }
        }
        for registry in &self.registries {
            for receipt in registry.receipts() {
                if !matches!(
                    receipt.request.scope,
                    MovementScope::ShareReDenomination { .. }
                ) {
                    continue;
                }
                let Some(&(settled_on, net_change)) =
                    settled_split_books.get(&receipt.request.event_id)
                else {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "名册存在无对应已入账拆股／缩股账簿的重新计值回执 {}",
                        receipt.request.event_id
                    )));
                };
                let receipt_net: i128 = receipt
                    .request
                    .changes
                    .iter()
                    .map(|change| change.change)
                    .sum();
                if receipt.request.day != settled_on || receipt_net != net_change {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "重新计值回执 {} 的日期或净额与拆股／缩股账簿不一致",
                        receipt.request.event_id
                    )));
                }
            }
        }
        company_system
            .validate_share_split_books(&self.share_splits)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        company_system
            .validate_stock_distribution_books(&self.stock_distributions)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        for book in &self.rights_offerings {
            book.validate()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if book.plan().approved_on > current_date {
                return Err(SessionCorporateActionsError::Invalid(
                    "配股批准日期晚于当前会话日期".into(),
                ));
            }
            if !self.registries.iter().any(|registry| {
                registry.stock() == &book.plan().stock && registry.issuer() == &book.plan().issuer
            }) {
                return Err(SessionCorporateActionsError::Invalid(
                    "配股账簿缺少匹配的完整股东名册".into(),
                ));
            }
            // 已过结算日仍停留 Closed 的账簿属于错过结算：显式失败。
            if book.status()
                == &crate::company::rights_offering::RightsOfferingStatus::Closed
                && current_date > book.plan().settlement_on
            {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "配股事件 {} 错过结算日",
                    book.plan().event_id
                )));
            }
            // 已入账账簿与名册非交易过户回执勾稽（零发行除外）。
            if let Some(credited_on) = book.credited_on() {
                let total = book
                    .settlement()
                    .map(|settlement| settlement.total_paid_shares)
                    .unwrap_or(0);
                if total > 0 {
                    let expected_event = format!("rights-offering:{}", book.plan().event_id);
                    let registry = self
                        .registries
                        .iter()
                        .find(|registry| registry.stock() == &book.plan().stock)
                        .expect("registry presence was checked above");
                    let Some(receipt) = registry.receipt_by_event(&expected_event) else {
                        return Err(SessionCorporateActionsError::Invalid(
                            "配股入账事实与股东名册非交易过户回执不一致".into(),
                        ));
                    };
                    let receipt_total = sum_nontrading_changes(&receipt.request.changes)?;
                    if receipt.request.day != credited_on
                        || !matches!(
                            receipt.request.scope,
                            MovementScope::NonTradingTransfer { .. }
                        )
                        || receipt_total != total
                    {
                        return Err(SessionCorporateActionsError::Invalid(
                            "配股入账事实与股东名册非交易过户回执不一致".into(),
                        ));
                    }
                }
            }
        }
        for queued in &self.rights_subscription_queue {
            if queued.event_id.trim().is_empty()
                || queued.requested_shares == 0
                || !accounts.contains_key(&queued.account)
                || !self
                    .rights_offerings
                    .iter()
                    .any(|book| book.plan().event_id == queued.event_id)
            {
                return Err(SessionCorporateActionsError::Invalid(
                    "配股认购排队事实非法".into(),
                ));
            }
        }
        // 拒绝回执勾稽：身份非空、账户与事件存在、日期有序、原因非空；
        // 同一 (event, account) 只能有一条回执，且不得同时存在已入账认购
        // （净认购唯一性：一个持有人要么入账、要么被拒，不能两者皆是）。
        let mut rejected_keys = BTreeSet::new();
        for receipt in &self.rejected_rights_subscriptions {
            if receipt.event_id.trim().is_empty()
                || receipt.requested_shares == 0
                || receipt.reason.trim().is_empty()
                || receipt.rejected_on < receipt.submitted_on
                || !accounts.contains_key(&receipt.account)
                || !self
                    .rights_offerings
                    .iter()
                    .any(|book| book.plan().event_id == receipt.event_id)
                || !rejected_keys.insert((receipt.event_id.clone(), receipt.account))
            {
                return Err(SessionCorporateActionsError::Invalid(
                    "配股认购拒绝回执事实非法".into(),
                ));
            }
            let subscribed = self
                .rights_offerings
                .iter()
                .find(|book| book.plan().event_id == receipt.event_id)
                .map(|book| {
                    book.subscriptions()
                        .iter()
                        .any(|record| record.holder == HolderId::Account(receipt.account))
                })
                .unwrap_or(false);
            if subscribed {
                return Err(SessionCorporateActionsError::Invalid(
                    "配股认购拒绝回执与已入账认购并存".into(),
                ));
            }
        }
        company_system
            .validate_rights_offering_books(&self.rights_offerings)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        company_system
            .validate_issuer_repurchase_books(&self.issuer_repurchases)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        let mut cancelled_books = BTreeMap::new();
        for book in &self.issuer_repurchases {
            book.validate()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if book.plan().approved_on > current_date {
                return Err(SessionCorporateActionsError::Invalid(
                    "回购批准日期晚于当前会话日期".into(),
                ));
            }
            if let Some(cancelled_on) = book.cancelled_on() {
                cancelled_books.insert(
                    format!("issuer-repurchase-cancellation:{}", book.plan().event_id),
                    (cancelled_on, book.cancelled_shares()),
                );
            }
        }
        // 回购注销回执反向勾稽：注销回执必须有对应 Cancelled 账簿且数量一致。
        for registry in &self.registries {
            for receipt in registry.receipts() {
                if !matches!(
                    receipt.request.scope,
                    MovementScope::IssuerRepurchaseCancellation { .. }
                ) {
                    continue;
                }
                let Some(&(cancelled_on, shares)) =
                    cancelled_books.get(&receipt.request.event_id)
                else {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "名册存在无对应回购账簿的注销回执 {}",
                        receipt.request.event_id
                    )));
                };
                let cancelled = receipt
                    .request
                    .changes
                    .iter()
                    .map(|change| change.change)
                    .sum::<i128>()
                    .unsigned_abs();
                let cancelled = u64::try_from(cancelled).map_err(|_| {
                    SessionCorporateActionsError::Invalid("回购注销回执数量非法".into())
                })?;
                if receipt.request.day != cancelled_on || cancelled != shares {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "回购注销回执 {} 的日期或数量与账簿不一致",
                        receipt.request.event_id
                    )));
                }
            }
        }
        let mut group_keys = BTreeSet::new();
        for group in &self.applied_ex_reference_groups {
            if group.date > current_date
                || group.reference.ex_date != group.date
                || group.reference.reference_price.cents() <= 0
                || !group_keys.insert((group.date, group.stock.clone()))
                || (group.cash_plan_ids.is_empty()
                    && group.stock_event_ids.is_empty()
                    && group.rights_event_ids.is_empty()
                    && group.split_event_ids.is_empty())
            {
                return Err(SessionCorporateActionsError::Invalid(
                    "已应用除权除息组事实非法或重复".into(),
                ));
            }
            let mut expected_cash: Vec<_> = self
                .dividends
                .iter()
                .filter(|book| {
                    book.plan().stock == group.stock
                        && book.plan().ex_dividend_on == group.date
                        && book.registration().is_some()
                })
                .map(|book| book.plan().plan_id.clone())
                .collect();
            expected_cash.sort();
            let mut actual_cash = group.cash_plan_ids.clone();
            actual_cash.sort();
            let mut expected_stock: Vec<_> = self
                .stock_distributions
                .iter()
                .filter(|book| {
                    book.plan().stock == group.stock
                        && book.plan().ex_rights_on == group.date
                        && book.registration().is_some()
                })
                .map(|book| book.plan().event_id.clone())
                .collect();
            expected_stock.sort();
            let mut actual_stock = group.stock_event_ids.clone();
            actual_stock.sort();
            let mut expected_rights: Vec<_> = self
                .rights_offerings
                .iter()
                .filter(|book| {
                    book.plan().stock == group.stock
                        && book.plan().ex_rights_on == group.date
                        && book.registration().is_some()
                        // 除权组分量谓词与首 tick 准备一致：实际认购为零、或
                        // 整数截位比例为零的微量认购都不构成组分量
                        // （forms_ex_rights_component 为两侧共同权威实现）。
                        && crate::company::rights_offering::forms_ex_rights_component(
                            book.subscriptions()
                                .iter()
                                .map(|record| record.paid_shares)
                                .sum::<u64>(),
                            book.entitlement()
                                .map(|receipt| receipt.issued_shares_before)
                                .unwrap_or(0),
                        )
                })
                .map(|book| book.plan().event_id.clone())
                .collect();
            expected_rights.sort();
            let mut actual_rights = group.rights_event_ids.clone();
            actual_rights.sort();
            let mut expected_splits: Vec<_> = self
                .share_splits
                .iter()
                .filter(|book| {
                    book.plan().stock == group.stock
                        && book.plan().ex_rights_on == group.date
                        && book.registration().is_some()
                })
                .map(|book| book.plan().event_id.clone())
                .collect();
            expected_splits.sort();
            let mut actual_splits = group.split_event_ids.clone();
            actual_splits.sort();
            if expected_cash != actual_cash
                || actual_cash.windows(2).any(|pair| pair[0] == pair[1])
                || expected_stock != actual_stock
                || actual_stock.windows(2).any(|pair| pair[0] == pair[1])
                || expected_rights != actual_rights
                || actual_rights.windows(2).any(|pair| pair[0] == pair[1])
                || expected_splits != actual_splits
                || actual_splits.windows(2).any(|pair| pair[0] == pair[1])
            {
                return Err(SessionCorporateActionsError::Invalid(
                    "已应用除权除息组与登记分红方案、送转、配股或拆股／缩股事件不一致".into(),
                ));
            }
        }
        let mut receipt_keys = BTreeSet::new();
        let mut account_receipt_keys = BTreeSet::new();
        for receipt in &self.account_gross_receipts {
            if receipt.paid_on > current_date {
                return Err(SessionCorporateActionsError::Invalid(
                    "账户到账日期晚于当前会话日期".into(),
                ));
            }
            if receipt.payment_id.trim().is_empty()
                || receipt.plan_id.trim().is_empty()
                || receipt.gross.cents() <= 0
                || !accounts.contains_key(&receipt.account)
                || !account_receipt_keys.insert((receipt.payment_id.clone(), receipt.account))
            {
                return Err(SessionCorporateActionsError::Invalid(
                    "账户 gross 到账凭证身份重复或事实非法".into(),
                ));
            }
            let book = self
                .dividends
                .iter()
                .find(|book| book.plan().plan_id == receipt.plan_id)
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("账户 gross 到账凭证缺少分红计划".into())
                })?;
            let tax_status = if self.dividend_tax_books.iter().any(|tax_book| {
                tax_book.account() == receipt.account && tax_book.stock() == &book.plan().stock
            }) {
                DividendTaxStatus::IndividualPublicMarket
            } else {
                DividendTaxStatus::TreatmentNotConfigured
            };
            if receipt.tax_status != tax_status {
                return Err(SessionCorporateActionsError::Invalid(
                    "账户股息税状态与显式税账配置不一致".into(),
                ));
            }
            let matches_payment = book.payments().iter().any(|payment| {
                payment.payment_id() == receipt.payment_id && payment.paid_on() == receipt.paid_on
                    && payment.outcomes().iter().any(|outcome| matches!(outcome,
                        crate::company::cash_dividend::HolderPaymentOutcome::Paid { holder: HolderId::Account(account), amount }
                        if account == &receipt.account && amount == &receipt.gross))
            });
            if !matches_payment {
                return Err(SessionCorporateActionsError::Invalid(
                    "账户 gross 到账凭证与分红成功付款回执不一致".into(),
                ));
            }
        }
        for receipt in &self.external_receipts {
            if receipt.paid_on > current_date {
                return Err(SessionCorporateActionsError::Invalid(
                    "External到账日期晚于当前会话日期".into(),
                ));
            }
            let HolderId::External(name) = &receipt.holder else {
                return Err(SessionCorporateActionsError::Invalid(
                    "外部收款凭证必须指向具名 External holder".into(),
                ));
            };
            if receipt.payment_id.trim().is_empty()
                || receipt.plan_id.trim().is_empty()
                || name.trim().is_empty()
                || receipt.gross.cents() <= 0
                || receipt.tax_status != DividendTaxStatus::TreatmentNotConfigured
                || !receipt_keys.insert((receipt.payment_id.clone(), receipt.holder.clone()))
            {
                return Err(SessionCorporateActionsError::Invalid(
                    "外部股东收款凭证身份重复或事实非法".into(),
                ));
            }
            let book = self
                .dividends
                .iter()
                .find(|book| book.plan().plan_id == receipt.plan_id)
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("外部股东收款凭证缺少分红计划".into())
                })?;
            let matches_payment = book.payments().iter().any(|payment| {
                payment.payment_id() == receipt.payment_id
                    && payment.paid_on() == receipt.paid_on
                    && payment.outcomes().iter().any(|outcome| {
                        matches!(outcome,
                        crate::company::cash_dividend::HolderPaymentOutcome::Paid { holder, amount }
                        if holder == &receipt.holder && amount == &receipt.gross)
                    })
            });
            if !matches_payment {
                return Err(SessionCorporateActionsError::Invalid(
                    "外部股东收款凭证与分红成功付款回执不一致".into(),
                ));
            }
        }
        for book in &self.dividends {
            for payment in book.payments() {
                for outcome in payment.outcomes() {
                    if let crate::company::cash_dividend::HolderPaymentOutcome::Paid {
                        holder: HolderId::Account(account),
                        amount,
                    } = outcome
                    {
                        let tax_status = if self.dividend_tax_books.iter().any(|tax_book| {
                            tax_book.account() == *account && tax_book.stock() == &book.plan().stock
                        }) {
                            DividendTaxStatus::IndividualPublicMarket
                        } else {
                            DividendTaxStatus::TreatmentNotConfigured
                        };
                        if !self.account_gross_receipts.iter().any(|receipt| {
                            receipt.plan_id == book.plan().plan_id
                                && receipt.payment_id == payment.payment_id()
                                && receipt.paid_on == payment.paid_on()
                                && &receipt.account == account
                                && &receipt.gross == amount
                                && receipt.tax_status == tax_status
                        }) {
                            return Err(SessionCorporateActionsError::Invalid(
                                "已到账 Account holder 缺少 gross 到账凭证或 tax未配置标记".into(),
                            ));
                        }
                    }
                    if let crate::company::cash_dividend::HolderPaymentOutcome::Paid {
                        holder: HolderId::External(_),
                        amount,
                    } = outcome
                    {
                        if !self.external_receipts.iter().any(|receipt| {
                            receipt.plan_id == book.plan().plan_id
                                && receipt.payment_id == payment.payment_id()
                                && receipt.paid_on == payment.paid_on()
                                && &receipt.holder == outcome.holder()
                                && &receipt.gross == amount
                        }) {
                            return Err(SessionCorporateActionsError::Invalid(
                                "已付款的 External holder 缺少具名到账凭证".into(),
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn configure_registry(
        &mut self,
        registry: ShareRegistry,
        accounts: &BTreeMap<AccountId, BTreeMap<StockCode, u64>>,
        issuers: &crate::company::identity::IssuerRegistry,
    ) -> Result<(), SessionCorporateActionsError> {
        Self::validate_registry(&registry, accounts, issuers, None)?;
        if self
            .registries
            .iter()
            .any(|known| known.stock() == registry.stock())
        {
            return Err(SessionCorporateActionsError::Invalid(
                "证券已经配置股东名册".into(),
            ));
        }
        self.registries.push(registry);
        self.registries
            .sort_by(|left, right| left.stock().cmp(right.stock()));
        Ok(())
    }

    /// 查询各已配置个人股息税账的当前未划收税额与资金不足原因。
    /// 每次日终收缴后未清余额仅在资金不足时存在，故 `needs_funds` 等价于余额非零；
    /// 本方法只读汇总，不落任何新事实，UI 呈现由后续批次接线。
    pub fn dividend_tax_outstanding_views(
        &self,
    ) -> Result<Vec<DividendTaxOutstandingView>, SessionCorporateActionsError> {
        let mut views = Vec::new();
        for book in &self.dividend_tax_books {
            let outstanding = book
                .outstanding_tax()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            let outstanding = ExactDividendTaxFraction {
                numerator: outstanding.numerator(),
                denominator: outstanding.denominator(),
            };
            let needs_funds = outstanding.numerator > 0;
            views.push(DividendTaxOutstandingView {
                account: book.account(),
                stock: book.stock().clone(),
                needs_funds,
                cause: if needs_funds {
                    DividendTaxOutstandingCause::InsufficientAvailableCash
                } else {
                    DividendTaxOutstandingCause::Cleared
                },
                outstanding,
            });
        }
        Ok(views)
    }

    pub(crate) fn configure_cash_dividend_tax_book(
        &mut self,
        account: crate::orderbook::AccountId,
        stock: crate::account::StockCode,
        profile: crate::company::cash_dividend_tax::DividendTaxProfile,
    ) -> Result<(), SessionCorporateActionsError> {
        let registry = self
            .registries
            .iter()
            .find(|registry| registry.stock() == &stock)
            .ok_or_else(|| {
                SessionCorporateActionsError::Invalid(format!(
                    "配置股息税前必须先提供 {stock:?} 的完整股东名册"
                ))
            })?;
        let holding = registry
            .holdings()
            .iter()
            .find(|holding| holding.holder == HolderId::Account(account))
            .ok_or_else(|| {
                SessionCorporateActionsError::Invalid(format!(
                    "账户 {account:?} 不是 {stock:?} 的登记股东，不能配置股息税"
                ))
            })?;
        // 装配期守卫：名册已有历史日结回执或已登记分红时拒绝配置。
        // 税账以名册日结事实重建 FIFO 批次；事后配置会丢失历史取得/处置事实，
        // 且首个历史回执无法满足“紧邻开账日下一自然日”，令后续每次日终永久失败。
        if !registry.receipts().is_empty()
            || self
                .dividends
                .iter()
                .any(|book| book.plan().stock == stock && book.registration().is_some())
        {
            return Err(SessionCorporateActionsError::Invalid(
                "股息税账只能在名册装配期配置：名册已有历史日结回执或已登记分红".into(),
            ));
        }
        if self
            .dividend_tax_books
            .iter()
            .any(|book| book.account() == account && book.stock() == &stock)
        {
            return Err(SessionCorporateActionsError::Invalid(
                "同一账户证券已配置股息税身份".into(),
            ));
        }
        let lots = holding
            .lots
            .iter()
            .map(|lot| crate::company::cash_dividend_tax::DividendTaxLot {
                id: format!("tax:{}", lot.id),
                qty: lot.qty,
                acquired_on: lot.acquired_on,
                source: crate::company::cash_dividend_tax::convert_tax_source(&lot.source),
                class: crate::company::cash_dividend_tax::convert_tax_class(&lot.restriction),
            })
            .collect();
        let book = crate::company::cash_dividend_tax::CashDividendTaxBook::new(
            account,
            stock,
            profile,
            registry.settled_on(),
            lots,
        )
        .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        self.dividend_tax_books.push(book);
        self.dividend_tax_books.sort_by(|left, right| {
            (left.account(), left.stock()).cmp(&(right.account(), right.stock()))
        });
        Ok(())
    }

    fn validate_registry(
        registry: &ShareRegistry,
        accounts: &BTreeMap<AccountId, BTreeMap<StockCode, u64>>,
        issuers: &crate::company::identity::IssuerRegistry,
        issuer_repurchase_account_id: Option<AccountId>,
    ) -> Result<(), SessionCorporateActionsError> {
        registry
            .validate()
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        if issuers.issuer_of(registry.stock()) != Some(registry.issuer())
            || issuers
                .get(registry.issuer())
                .map(|issuer| issuer.issued_shares)
                != Some(registry.issued_shares())
        {
            return Err(SessionCorporateActionsError::Invalid(
                "证券、发行人及发行股数必须与公司身份完全一致".into(),
            ));
        }
        let mut seen_accounts = BTreeSet::new();
        let mut shares_by_account = BTreeMap::new();
        for holding in registry.holdings() {
            if let HolderId::Account(id) = &holding.holder {
                let id = *id;
                if !seen_accounts.insert(id) {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "账户 {id:?} 重复出现在股东名册"
                    )));
                }
                let shares = holding
                    .lots
                    .iter()
                    .try_fold(0_u64, |total, lot| total.checked_add(lot.qty))
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("名册股份数量溢出".into())
                    })?;
                shares_by_account.insert(id, shares);
            }
        }
        for (account, positions) in accounts {
            // 发行人回购账户的名册身份是 IssuerTreasury（专户），不按 Account 持有人
            // 勾稽——其一致性由下方回购批次勾稽承担。
            if issuer_repurchase_account_id == Some(*account) {
                continue;
            }
            let actual = positions.get(registry.stock()).copied().unwrap_or(0);
            if actual != shares_by_account.get(account).copied().unwrap_or(0) {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "账户 {account:?} 的实际持仓与名册不一致：账户 {actual} 股，名册 {} 股",
                    shares_by_account.get(account).copied().unwrap_or(0)
                )));
            }
        }
        if shares_by_account
            .keys()
            .any(|account| !accounts.contains_key(account))
        {
            return Err(SessionCorporateActionsError::Invalid(
                "名册引用了不存在的账户".into(),
            ));
        }
        Ok(())
    }

    fn settle_dividend_tax(
        &mut self,
        accounts: &mut super::account_book::AccountBook,
    ) -> Result<(), SessionCorporateActionsError> {
        let book_keys: Vec<_> = self
            .dividend_tax_books
            .iter()
            .map(|book| (book.account(), book.stock().clone()))
            .collect();
        for (account, stock) in book_keys {
            let tax_index = self
                .dividend_tax_books
                .iter()
                .position(|item| item.account() == account && *item.stock() == stock)
                .expect("tax book key came from the same collection");
            for dividend_book in &self.dividends {
                if dividend_book.plan().stock != stock {
                    continue;
                }
                let Some(snapshot) = dividend_book.registration() else {
                    continue;
                };
                if !snapshot
                    .holdings()
                    .iter()
                    .any(|holding| holding.holder == HolderId::Account(account))
                {
                    continue;
                }
                let index = tax_index;
                let event_id = format!("dividend-tax:{}", snapshot.event_id());
                let settled_on = self.dividend_tax_books[index].settled_on();
                if settled_on < snapshot.registered_on() {
                    continue;
                }
                if self.dividend_tax_books[index]
                    .dividend_event_by_id(&event_id)
                    .is_none()
                {
                    self.dividend_tax_books[index]
                        .register_dividend(
                            event_id,
                            snapshot.registered_on(),
                            crate::company::cash_dividend_tax::ExactDividendTaxAmount::new(
                                i128::from(
                                    dividend_book
                                        .entitlements()
                                        .map_err(|error| {
                                            SessionCorporateActionsError::Invalid(error.to_string())
                                        })?
                                        .iter()
                                        .find(|entitlement| {
                                            entitlement.holder == HolderId::Account(account)
                                        })
                                        .map(|entitlement| entitlement.gross.cents())
                                        .unwrap_or(0),
                                ),
                                snapshot
                                    .holdings()
                                    .iter()
                                    .find(|holding| holding.holder == HolderId::Account(account))
                                    .and_then(|holding| {
                                        holding.lots.iter().try_fold(0_u64, |total, lot| {
                                            total.checked_add(lot.qty)
                                        })
                                    })
                                    .unwrap_or(0),
                            )
                            .map_err(|error| {
                                SessionCorporateActionsError::Invalid(error.to_string())
                            })?,
                        )
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                }
            }

            let payment_keys: Vec<_> = self
                .dividends
                .iter()
                .flat_map(|book| {
                    book.payments().iter().map(move |payment| {
                        (book.plan().plan_id.clone(), payment.payment_id().to_owned())
                    })
                })
                .collect();
            for (plan_id, payment_id) in payment_keys {
                let Some(receipt) = self.account_gross_receipts.iter().find(|receipt| {
                    receipt.account == account
                        && receipt.plan_id == plan_id
                        && receipt.payment_id == payment_id
                }) else {
                    continue;
                };
                let index = tax_index;
                let registration_event_id = format!("dividend-tax:{}", plan_id);
                let settled_on = self.dividend_tax_books[index].settled_on();
                if settled_on < receipt.paid_on {
                    continue;
                }
                if self.dividend_tax_books[index]
                    .payment_by_id(&payment_id)
                    .is_none()
                    && self.dividend_tax_books[index]
                        .dividend_event_by_id(&registration_event_id)
                        .is_some()
                {
                    self.dividend_tax_books[index]
                        .record_payment(
                            &registration_event_id,
                            payment_id.clone(),
                            receipt.paid_on,
                            receipt.gross,
                            format!("account-gross:{payment_id}"),
                        )
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                }
            }

            let index = tax_index;
            let settled_on = self.dividend_tax_books[index].settled_on();
            let outstanding = self.dividend_tax_books[index]
                .outstanding_tax()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            // 零税额且当日无新付款/处置时不落收缴回执，消除逐日零税回执的存档膨胀；
            // 有付款或处置事件的日照常留痕，未清税额由后续日终继续追缴。
            // 同日幂等重放由 collect_due 对既有 event_id 的事实比较满足。
            if outstanding.numerator() == 0
                && !self.dividend_tax_books[index].has_payment_on(settled_on)
                && !self.dividend_tax_books[index].has_disposition_on(settled_on)
            {
                continue;
            }
            let available_cash = accounts
                .get(&account)
                .map(|account| account.cash())
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid(format!("股息税账户 {account:?} 不存在"))
                })?;
            let collection_id =
                format!("dividend-tax-collect:{account:?}:{}:{settled_on}", stock.0);
            let collection = self.dividend_tax_books[index]
                .collect_due(collection_id, settled_on, available_cash)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if collection.collected.cents() > 0 {
                accounts
                    .get_mut(&account)
                    .expect("the account existence was checked above")
                    .debit_cash(collection.collected)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::StockCode,
        calendar::CivilDate,
        company::{
            share_registry::{AcquisitionSource, ShareHolding, ShareLot, ShareRestriction},
            CompanyId, CompanyKind, CompanySpec, IndustryId,
        },
    };

    fn d(year: i32, month: u8, day: u8) -> CivilDate {
        CivilDate::from_ymd(year, month, day).unwrap()
    }

    #[test]
    fn registry_requires_exact_issuer_security_and_real_account_positions() {
        let issuer = CompanyId("co".into());
        let code = StockCode("600000".into());
        let issuers = crate::company::identity::IssuerRegistry::new(vec![CompanySpec {
            id: issuer.clone(),
            name: "公司".into(),
            kind: CompanyKind::Industrial,
            industry: IndustryId("industry".into()),
            listed_stock: Some(code.clone()),
            issued_shares: 10,
            group_parent: None,
        }])
        .unwrap();
        let lot = ShareLot {
            id: "initial-account-1".into(),
            qty: 6,
            acquired_on: d(2026, 1, 1),
            source: AcquisitionSource::InitialAllocation {
                evidence: "explicit setup facts".into(),
            },
            restriction: ShareRestriction::Unrestricted,
        };
        let registry = ShareRegistry::new(
            code.clone(),
            issuer,
            10,
            d(2026, 1, 1),
            vec![
                ShareHolding {
                    holder: HolderId::Account(AccountId(1)),
                    lots: vec![lot],
                },
                ShareHolding {
                    holder: HolderId::External("named-holder".into()),
                    lots: vec![ShareLot {
                        id: "initial-external".into(),
                        qty: 4,
                        acquired_on: d(2026, 1, 1),
                        source: AcquisitionSource::InitialAllocation {
                            evidence: "explicit setup facts".into(),
                        },
                        restriction: ShareRestriction::Unrestricted,
                    }],
                },
            ],
        )
        .unwrap();
        let mut actions = SessionCorporateActions::default();
        assert!(actions
            .configure_registry(
                registry.clone(),
                &BTreeMap::from([(AccountId(1), BTreeMap::from([(code.clone(), 5)]))]),
                &issuers
            )
            .is_err());
        assert!(actions.registries.is_empty());
        actions
            .configure_registry(
                registry,
                &BTreeMap::from([(AccountId(1), BTreeMap::from([(code, 6)]))]),
                &issuers,
            )
            .unwrap();
        assert_eq!(actions.registries.len(), 1);
    }

    #[test]
    fn registry_day_end_advances_calendar_days_without_inventing_unlisted_holders() {
        let issuer = CompanyId("co".into());
        let code = StockCode("600000".into());
        let issuers = crate::company::identity::IssuerRegistry::new(vec![CompanySpec {
            id: issuer.clone(),
            name: "公司".into(),
            kind: CompanyKind::Industrial,
            industry: IndustryId("industry".into()),
            listed_stock: Some(code.clone()),
            issued_shares: 10,
            group_parent: None,
        }])
        .unwrap();
        let registry = ShareRegistry::new(
            code.clone(),
            issuer,
            10,
            d(2026, 10, 2),
            vec![
                ShareHolding {
                    holder: HolderId::Account(AccountId(1)),
                    lots: vec![ShareLot {
                        id: "known-lot".into(),
                        qty: 6,
                        acquired_on: d(2026, 10, 2),
                        source: AcquisitionSource::InitialAllocation {
                            evidence: "explicit setup fact".into(),
                        },
                        restriction: ShareRestriction::Unrestricted,
                    }],
                },
                ShareHolding {
                    holder: HolderId::External("named-holder".into()),
                    lots: vec![ShareLot {
                        id: "known-external-lot".into(),
                        qty: 4,
                        acquired_on: d(2026, 10, 2),
                        source: AcquisitionSource::InitialAllocation {
                            evidence: "explicit setup fact".into(),
                        },
                        restriction: ShareRestriction::Unrestricted,
                    }],
                },
            ],
        )
        .unwrap();
        let mut actions = SessionCorporateActions {
            registries: vec![registry],
            dividends: vec![],
            account_gross_receipts: vec![],
            external_receipts: vec![],
            dividend_tax_books: vec![],
            stock_distributions: vec![],
            share_splits: vec![],
            rights_offerings: vec![],
            rights_subscription_queue: vec![],
            rejected_rights_subscriptions: vec![],
            issuer_repurchases: vec![],
            applied_ex_reference_groups: vec![],
        };
        let accounts = BTreeMap::from([(AccountId(1), BTreeMap::from([(code, 6)]))]);
        actions
            .close_registries_through(d(2026, 10, 5), &accounts, &BTreeMap::new(), None)
            .unwrap();
        assert_eq!(actions.registries[0].settled_on(), d(2026, 10, 5));
        assert_eq!(actions.registries[0].holdings().len(), 2);
        assert_eq!(
            actions.registries[0]
                .holdings()
                .iter()
                .map(|holding| holding.lots.iter().map(|lot| lot.qty).sum::<u64>())
                .sum::<u64>(),
            10
        );
        assert!(SessionCorporateActions::validate_registry(
            &actions.registries[0],
            &accounts,
            &issuers,
            None
        )
        .is_ok());
    }
}
