//! 玩家公司行为提案（N2b，2026-10-08 用户决策「持仓即可、直接生效」）。
//!
//! 真实 A 股的公司决议由发行人依治理程序作出，证券投资者不能发起；本入口按
//! 用户产品决策登记为**游戏化简化**（docs/trading-rules.md「玩家提案」节）：
//! 玩家对自己有持仓的公司可直接发起六类行为提案（现金分红/送转/配股/增发/
//! 回购/拆股），前置校验只有一条——发起账户对该发行人上市证券的**账户持仓**
//! （任意数量 > 0，名册外持仓也算持有）；随后构造显式方案并**直接调用既有
//! `approve_*` 入口**完成全部制度校验（可分配利润、法定事实、碰撞预检、新局
//! 开关等），不复制任何校验逻辑，拒绝错误原样上抛并附加 F 批错误四分类。
//!
//! 日程推导与偏好自动提案同一口径（`company::simple::preferences`）：批准日 =
//! 公告日 = 当前自然日；权益登记日为公告日后的首个交易日且不与公告日同日
//! （保证 typed 公告通道在公告日日结仍处于 Announced 状态、照常发布）；除息/
//! 除权日为登记日次一交易日。配股缴款期与回购窗口由提案参数显式给出。

use super::*;
use crate::company::CompanyErrorClass;

/// 玩家提案类别（六类；配股与增发在机制层共用配股通道、按模式区分）。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum PlayerProposalKind {
    /// 现金分红。
    CashDividend,
    /// 送转（股票股利／资本公积转增）。
    StockDistribution,
    /// 配股：面向全体股东按持股比例配售。
    RightsOffering,
    /// 增发：定向配售（提案玩家本人承购）。
    SecondaryOffering,
    /// 发行人回购。
    IssuerRepurchase,
    /// 拆股／缩股（重新计值）。
    ShareSplit,
}

/// 玩家提案的最小参数集（六类；公司身份、金额与比例，日程由引擎推导）。
/// 金额沿用 `Money` 规范有符号十进制分字符串（ADR-0031）；股数与比例沿用
/// 规范 u64 十进制字符串，与既有公司契约 wire 口径一致。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum PlayerCompanyProposal {
    /// 现金分红：每股税前红利（正数）。
    CashDividend {
        company: CompanyId,
        gross_per_share: Money,
    },
    /// 送转：种类（送股／转增）与每股送转比例（百万分之一股/股，如 10 送 10 =
    /// 1_000_000）。
    StockDistribution {
        company: CompanyId,
        kind: crate::company::stock_distribution::StockDistributionKind,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        #[ts(type = "string")]
        shares_per_existing_share_micros: u64,
    },
    /// 配股：发行价、每 1 股配售比例（百万分之一股）与缴款期交易日数（≥ 1）。
    RightsOffering {
        company: CompanyId,
        price_per_share: Money,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        #[ts(type = "string")]
        shares_per_existing_share_micros: u64,
        payment_days: u32,
    },
    /// 增发（定向）：发行价与定向股数（定向对象为提案玩家本人承购，不设锁定期；
    /// ADR-0039 第 5 条「玩家承购」去向）。
    SecondaryOffering {
        company: CompanyId,
        price_per_share: Money,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        #[ts(type = "string")]
        shares: u64,
    },
    /// 发行人回购：价格上限、获批额度（合成资金总额）、数量上限、窗口交易日数
    /// （≥ 1，窗口起点为公告日后首个交易日）与用途。
    IssuerRepurchase {
        company: CompanyId,
        price_cap_per_share: Money,
        total_budget: Money,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        #[ts(type = "string")]
        max_shares: u64,
        window_trading_days: u32,
        purpose: crate::company::issuer_repurchase::RepurchasePurpose,
    },
    /// 拆股／缩股：方向与整数换算比例（≥ 2）。
    ShareSplit {
        company: CompanyId,
        direction: crate::company::share_split::ShareSplitDirection,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        #[ts(type = "string")]
        ratio: u64,
    },
}

impl PlayerCompanyProposal {
    pub fn kind(&self) -> PlayerProposalKind {
        match self {
            Self::CashDividend { .. } => PlayerProposalKind::CashDividend,
            Self::StockDistribution { .. } => PlayerProposalKind::StockDistribution,
            Self::RightsOffering { .. } => PlayerProposalKind::RightsOffering,
            Self::SecondaryOffering { .. } => PlayerProposalKind::SecondaryOffering,
            Self::IssuerRepurchase { .. } => PlayerProposalKind::IssuerRepurchase,
            Self::ShareSplit { .. } => PlayerProposalKind::ShareSplit,
        }
    }

    pub fn company(&self) -> &CompanyId {
        match self {
            Self::CashDividend { company, .. }
            | Self::StockDistribution { company, .. }
            | Self::RightsOffering { company, .. }
            | Self::SecondaryOffering { company, .. }
            | Self::IssuerRepurchase { company, .. }
            | Self::ShareSplit { company, .. } => company,
        }
    }
}

/// 提案受理回执：方案身份（现金分红为 plan_id，其余为 event_id）与批准/公告日。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PlayerProposalReceipt {
    pub kind: PlayerProposalKind,
    /// 受理的方案身份：现金分红为 plan_id，其余为 event_id。
    pub identity: String,
    pub approved_on: CivilDate,
    pub announced_on: CivilDate,
}

/// 玩家提案失败（三类显式分流：无持仓拒绝／制度拒绝）。
///
/// 制度拒绝携带 F 批错误四分类（`CompanyErrorClass`）：构造期参数域违反为
/// `InvalidInput`；新局开关关闭为 `UnsupportedOperation`；既有 approve_* 制度
/// 校验拒绝（可分配利润、法定事实、碰撞预检等）为 `BusinessCondition`；结构
/// 性内部不一致为 `SystemState`。分类是附加面，不替代原样错误信息。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum PlayerProposalError {
    /// 无持仓拒绝：发起账户对该发行人上市证券的账户持仓为 0（前置校验未通过，
    /// 不进入任何制度校验）。
    NoHolding {
        company: CompanyId,
        stock: StockCode,
        detail: String,
    },
    /// 制度拒绝（含构造期参数域拒绝）：原样错误信息 + F 批四分类。
    Institutional {
        kind: PlayerProposalKind,
        detail: String,
        class: CompanyErrorClass,
    },
}

impl PlayerProposalError {
    fn rules(
        kind: PlayerProposalKind,
        class: CompanyErrorClass,
        detail: impl Into<String>,
    ) -> Self {
        Self::Institutional {
            kind,
            class,
            detail: detail.into(),
        }
    }
}

/// 玩家提案的宿主 wire 结果（三类显式：受理／制度拒绝／无持仓拒绝）。
/// 业务结果**不抛错**（避免把制度拒绝伪装成传输故障），由严格 parser 消费。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum PlayerProposalOutcome {
    Accepted {
        receipt: PlayerProposalReceipt,
    },
    NoHolding {
        company: CompanyId,
        stock: StockCode,
        detail: String,
    },
    InstitutionalRejection {
        kind: PlayerProposalKind,
        detail: String,
        class: CompanyErrorClass,
    },
}

impl From<Result<PlayerProposalReceipt, PlayerProposalError>> for PlayerProposalOutcome {
    fn from(result: Result<PlayerProposalReceipt, PlayerProposalError>) -> Self {
        match result {
            Ok(receipt) => Self::Accepted { receipt },
            Err(PlayerProposalError::NoHolding {
                company,
                stock,
                detail,
            }) => Self::NoHolding {
                company,
                stock,
                detail,
            },
            Err(PlayerProposalError::Institutional {
                kind,
                detail,
                class,
            }) => Self::InstitutionalRejection {
                kind,
                detail,
                class,
            },
        }
    }
}

/// 玩家提案的确定性方案身份（同日多次提案按既有账簿计数递增，账簿只增不减，
/// 恢复保留既有身份，不重推导）。
fn player_proposal_identity(
    prefix: &str,
    company: &CompanyId,
    day: CivilDate,
    index: usize,
) -> String {
    format!("player-proposal:{}:{prefix}:{day}:{index}", company.0)
}

/// 名册中非库藏股（IssuerTreasury 除外）的合计股数。与
/// `GameSession::approve_cash_dividend` 的既有口径逐字一致；该入口受理时按同一
/// 口径复核决议总额，任何漂移都会被显式拒绝（不会静默错账）。
fn non_treasury_registry_shares(
    registry: &crate::company::share_registry::ShareRegistry,
) -> Result<u64, String> {
    registry
        .holdings()
        .iter()
        .filter(|holding| {
            !matches!(
                &holding.holder,
                crate::company::share_registry::HolderId::IssuerTreasury
            )
        })
        .try_fold(0_u64, |total, holding| {
            holding
                .lots
                .iter()
                .try_fold(total, |sum, lot| sum.checked_add(lot.qty))
        })
        .ok_or_else(|| "名册非库藏股股数溢出".to_string())
}

impl GameSession {
    /// 玩家提案入口（N2b，2026-10-08 用户决策）：前置校验 = 发起账户对该发行人
    /// 上市证券的**账户持仓**（任意数量 > 0；名册外持仓也算持有）；随后构造
    /// 显式方案并**直接调用既有 `approve_*` 入口**完成全部制度校验，不复制校验
    /// 逻辑，拒绝错误原样上抛并附加 F 批四分类。真实 A 股投资者不能发起公司
    /// 决议，本入口为登记在案的游戏化简化。
    pub fn propose_company_action(
        &mut self,
        account: AccountId,
        proposal: PlayerCompanyProposal,
    ) -> Result<PlayerProposalReceipt, PlayerProposalError> {
        let kind = proposal.kind();
        let company = proposal.company().clone();

        // —— 前置校验（唯一新增规则）：账户持仓 > 0 ——
        let Some(account_state) = self.state.accounts.get(&account) else {
            return Err(PlayerProposalError::rules(
                kind,
                CompanyErrorClass::InvalidInput,
                format!("发起提案的账户 {account:?} 不存在"),
            ));
        };
        let issuer = self
            .state
            .company_system
            .issuers()
            .get(&company)
            .cloned()
            .ok_or_else(|| {
                PlayerProposalError::rules(
                    kind,
                    CompanyErrorClass::InvalidInput,
                    format!("未知公司 {}", company.0),
                )
            })?;
        let Some(stock) = issuer.listed_stock.clone() else {
            return Err(PlayerProposalError::rules(
                kind,
                CompanyErrorClass::BusinessCondition,
                format!("公司 {} 尚未上市，无公司行为通道", company.0),
            ));
        };
        let holding_qty = account_state
            .position(&stock)
            .map(|position| u64::from(position.qty()))
            .unwrap_or(0);
        if holding_qty == 0 {
            return Err(PlayerProposalError::NoHolding {
                company,
                stock: stock.clone(),
                detail: format!(
                    "账户 {account:?} 对发行人上市证券 {stock:?} 的账户持仓为 0；\
                     玩家提案要求任意数量 > 0 的持仓（名册外持仓也算持有）"
                ),
            });
        }

        // —— 日程推导上下文（与偏好自动提案同一口径）——
        let today = self.civil_date();
        let Some(exchange) = self.state.civil_clock.stock_exchange(&stock) else {
            return Err(PlayerProposalError::rules(
                kind,
                CompanyErrorClass::SystemState,
                format!("提案缺少 {stock:?} 的交易所日历映射"),
            ));
        };
        let calendar = self.state.civil_clock.calendar().clone();
        let approval_reference = format!("player-proposal:{account:?}");

        match proposal {
            PlayerCompanyProposal::CashDividend {
                gross_per_share, ..
            } => {
                if gross_per_share <= Money::ZERO {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::InvalidInput,
                        "现金分红每股税前红利必须为正数（分）",
                    ));
                }
                let registry = self.registry_for_proposal(&company, &stock, "现金分红", kind)?;
                let eligible_shares = non_treasury_registry_shares(registry).map_err(|detail| {
                    PlayerProposalError::rules(kind, CompanyErrorClass::SystemState, detail)
                })?;
                let (registered_on, ex_on) =
                    self.proposal_ex_dates(&calendar, exchange, today, kind)?;
                let distributable = self
                    .state
                    .company_system
                    .distributable_profit(&company)
                    .map_err(|error| {
                        PlayerProposalError::rules(
                            kind,
                            CompanyErrorClass::BusinessCondition,
                            error.to_string(),
                        )
                    })?;
                let authorized = distributable
                    .available_for_distribution
                    .to_money()
                    .map_err(|error| {
                        PlayerProposalError::rules(
                            kind,
                            CompanyErrorClass::SystemState,
                            error.to_string(),
                        )
                    })?;
                let legal = self
                    .state
                    .company_system
                    .dividend_legal_facts(&company)
                    .map_err(|error| {
                        PlayerProposalError::rules(
                            kind,
                            CompanyErrorClass::SystemState,
                            error.to_string(),
                        )
                    })?
                    .ok_or_else(|| {
                        PlayerProposalError::rules(
                            kind,
                            CompanyErrorClass::BusinessCondition,
                            "现金分红提案需要先显式绑定公司注册资本法定事实",
                        )
                    })?;
                let index = self.state.corporate_actions.dividends.len();
                let plan_id = player_proposal_identity("dividend", &company, today, index);
                let plan = crate::company::cash_dividend::CashDividendPlan::new(
                    plan_id.clone(),
                    company.clone(),
                    stock.clone(),
                    exchange,
                    crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
                    today,
                    today,
                    registered_on,
                    ex_on,
                    ex_on,
                    gross_per_share,
                    authorized,
                    &calendar,
                )
                .map_err(|error| {
                    PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::InvalidInput,
                        error.to_string(),
                    )
                })?;
                let total_gross = crate::accounting::AccountingAmount::from_cents(
                    i128::from(gross_per_share.cents()) * i128::from(eligible_shares),
                );
                let declaration = crate::company::DividendDeclaration {
                    plan_id: plan_id.clone(),
                    approved_on: today,
                    total_gross,
                    registered_capital: legal.registered_capital,
                };
                if let Err(error) = self.approve_cash_dividend(declaration, plan) {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::BusinessCondition,
                        error.to_string(),
                    ));
                }
                Ok(PlayerProposalReceipt {
                    kind,
                    identity: plan_id,
                    approved_on: today,
                    announced_on: today,
                })
            }
            PlayerCompanyProposal::StockDistribution {
                kind: distribution_kind,
                shares_per_existing_share_micros,
                ..
            } => {
                if shares_per_existing_share_micros == 0 {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::InvalidInput,
                        "送转每股比例必须 ≥ 1 百万分比",
                    ));
                }
                let registry = self.registry_for_proposal(&company, &stock, "送转", kind)?;
                let eligible_shares = non_treasury_registry_shares(registry).map_err(|detail| {
                    PlayerProposalError::rules(kind, CompanyErrorClass::SystemState, detail)
                })?;
                let proposed_total = u128::from(eligible_shares)
                    .checked_mul(u128::from(shares_per_existing_share_micros))
                    .map(|product| product / u128::from(1_000_000_u64))
                    .ok_or_else(|| {
                        PlayerProposalError::rules(
                            kind,
                            CompanyErrorClass::SystemState,
                            "送转提案新增股数推导溢出",
                        )
                    })?;
                let new_shares = match u64::try_from(proposed_total) {
                    Ok(shares) if shares > 0 => shares,
                    _ => {
                        return Err(PlayerProposalError::rules(
                            kind,
                            CompanyErrorClass::InvalidInput,
                            format!(
                                "送转比例 {shares_per_existing_share_micros} 百万分比摊到\
                                 {eligible_shares} 股不足一股，无法构造方案"
                            ),
                        ));
                    }
                };
                let (registered_on, ex_on) =
                    self.proposal_ex_dates(&calendar, exchange, today, kind)?;
                let index = self.state.corporate_actions.stock_distributions.len();
                let event_id =
                    player_proposal_identity("stock-distribution", &company, today, index);
                let plan = crate::company::stock_distribution::StockDistributionEventPlan {
                    event_id: event_id.clone(),
                    approval_reference,
                    issuer: company.clone(),
                    stock: stock.clone(),
                    exchange,
                    kind: distribution_kind,
                    approved_on: today,
                    announced_on: today,
                    registered_on,
                    ex_rights_on: ex_on,
                    shares_per_existing_share_micros,
                    approved_total_new_shares: new_shares,
                };
                if let Err(error) = self.approve_stock_distribution(plan) {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::BusinessCondition,
                        error.to_string(),
                    ));
                }
                Ok(PlayerProposalReceipt {
                    kind,
                    identity: event_id,
                    approved_on: today,
                    announced_on: today,
                })
            }
            PlayerCompanyProposal::RightsOffering {
                price_per_share,
                shares_per_existing_share_micros,
                payment_days,
                ..
            } => {
                if price_per_share <= Money::ZERO {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::InvalidInput,
                        "配股发行价必须为正数（分）",
                    ));
                }
                if shares_per_existing_share_micros == 0 {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::InvalidInput,
                        "配股比例必须 ≥ 1 百万分比",
                    ));
                }
                let receipt = self.propose_rights_style(
                    kind,
                    &company,
                    &stock,
                    exchange,
                    &calendar,
                    today,
                    approval_reference,
                    price_per_share,
                    crate::company::rights_offering::RightsOfferingMode::RightsToAllShareholders {
                        shares_per_existing_share_micros,
                    },
                    payment_days,
                )?;
                Ok(receipt)
            }
            PlayerCompanyProposal::SecondaryOffering {
                price_per_share,
                shares,
                ..
            } => {
                if price_per_share <= Money::ZERO {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::InvalidInput,
                        "增发发行价必须为正数（分）",
                    ));
                }
                if shares == 0 {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::InvalidInput,
                        "增发定向股数必须为正数",
                    ));
                }
                let receipt = self.propose_rights_style(
                    kind,
                    &company,
                    &stock,
                    exchange,
                    &calendar,
                    today,
                    approval_reference,
                    price_per_share,
                    crate::company::rights_offering::RightsOfferingMode::DirectedPlacement {
                        targets: vec![
                            crate::company::rights_offering::DirectedPlacementTarget::NamedHolder {
                                holder: crate::company::share_registry::HolderId::Account(account),
                                shares,
                                lock_until: None,
                            },
                        ],
                    },
                    // 定向增发无面向全体股东的缴款期参数化默认：沿用与配股一致的
                    // 最短缴款期（1 个交易日），由显式常量给出。
                    1,
                )?;
                Ok(receipt)
            }
            PlayerCompanyProposal::IssuerRepurchase {
                price_cap_per_share,
                total_budget,
                max_shares,
                window_trading_days,
                purpose,
                ..
            } => {
                if price_cap_per_share <= Money::ZERO || total_budget <= Money::ZERO {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::InvalidInput,
                        "回购价格上限与获批额度必须为正数（分）",
                    ));
                }
                if max_shares == 0 {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::InvalidInput,
                        "回购数量上限必须为正数",
                    ));
                }
                if window_trading_days == 0 {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::InvalidInput,
                        "回购窗口必须覆盖至少 1 个交易日",
                    ));
                }
                // 窗口起点 = 公告日后首个交易日；截止日再推进 window_trading_days−1
                // 个交易日（含起点共 window_trading_days 个交易日）。
                let window_start_on =
                    self.proposal_registered_on(&calendar, exchange, today, kind)?;
                let mut window_deadline_on = window_start_on;
                for _ in 1..window_trading_days {
                    window_deadline_on = self.next_trading_day_for_proposal(
                        &calendar,
                        exchange,
                        window_deadline_on,
                        kind,
                    )?;
                }
                let index = self.state.corporate_actions.issuer_repurchases.len();
                let event_id = player_proposal_identity("repurchase", &company, today, index);
                let plan = crate::company::issuer_repurchase::IssuerRepurchasePlan {
                    event_id: event_id.clone(),
                    approval_reference,
                    issuer: company.clone(),
                    stock: stock.clone(),
                    exchange,
                    approved_on: today,
                    announced_on: today,
                    window_start_on,
                    window_deadline_on,
                    price_cap_per_share,
                    total_budget,
                    max_shares,
                    purpose,
                };
                if let Err(error) = self.approve_issuer_repurchase(plan) {
                    // 分类依据当前开关状态（非错误文本启发式）：开关关闭 → 未支持
                    // 操作；开关开启时的拒绝为业务条件拒绝（可分配/10% 上限/唯一
                    // 未完成方案/名册等既有制度校验）。
                    let class = if self.state.setup.issuer_repurchase_enabled {
                        CompanyErrorClass::BusinessCondition
                    } else {
                        CompanyErrorClass::UnsupportedOperation
                    };
                    return Err(PlayerProposalError::rules(kind, class, error.to_string()));
                }
                Ok(PlayerProposalReceipt {
                    kind,
                    identity: event_id,
                    approved_on: today,
                    announced_on: today,
                })
            }
            PlayerCompanyProposal::ShareSplit {
                direction, ratio, ..
            } => {
                if ratio < 2 {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::InvalidInput,
                        "拆股／缩股整数换算比例必须 ≥ 2",
                    ));
                }
                let (registered_on, ex_on) =
                    self.proposal_ex_dates(&calendar, exchange, today, kind)?;
                let index = self.state.corporate_actions.share_splits.len();
                let event_id = player_proposal_identity("split", &company, today, index);
                let plan = crate::company::share_split::ShareSplitEventPlan {
                    event_id: event_id.clone(),
                    approval_reference,
                    issuer: company.clone(),
                    stock: stock.clone(),
                    exchange,
                    direction,
                    ratio,
                    approved_on: today,
                    announced_on: today,
                    registered_on,
                    ex_rights_on: ex_on,
                };
                if let Err(error) = self.approve_share_split(plan) {
                    return Err(PlayerProposalError::rules(
                        kind,
                        CompanyErrorClass::BusinessCondition,
                        error.to_string(),
                    ));
                }
                Ok(PlayerProposalReceipt {
                    kind,
                    identity: event_id,
                    approved_on: today,
                    announced_on: today,
                })
            }
        }
    }

    /// 配股／增发两类提案的共用构造：登记日 = 公告日后首个交易日（不与公告日
    /// 同日），缴款期日程由 `derive_schedule` 按既有口径推导，随后直接调用
    /// `approve_rights_offering`。
    #[allow(clippy::too_many_arguments)]
    fn propose_rights_style(
        &mut self,
        kind: PlayerProposalKind,
        company: &CompanyId,
        stock: &StockCode,
        exchange: crate::calendar::CalendarExchange,
        calendar: &TradingCalendar,
        today: CivilDate,
        approval_reference: String,
        price_per_share: Money,
        mode: crate::company::rights_offering::RightsOfferingMode,
        payment_days: u32,
    ) -> Result<PlayerProposalReceipt, PlayerProposalError> {
        if payment_days == 0 {
            return Err(PlayerProposalError::rules(
                kind,
                CompanyErrorClass::InvalidInput,
                "缴款期必须覆盖至少 1 个交易日",
            ));
        }
        let registered_on = self.proposal_registered_on(calendar, exchange, today, kind)?;
        let index = self.state.corporate_actions.rights_offerings.len();
        let event_id = player_proposal_identity(
            if matches!(kind, PlayerProposalKind::SecondaryOffering) {
                "offering"
            } else {
                "rights"
            },
            company,
            today,
            index,
        );
        let mut plan = crate::company::rights_offering::RightsOfferingEventPlan {
            event_id: event_id.clone(),
            approval_reference,
            issuer: company.clone(),
            stock: stock.clone(),
            exchange,
            approved_on: today,
            announced_on: today,
            registered_on,
            // derive_schedule 会按登记日与缴款期覆写以下日期。
            payment_start_on: registered_on,
            payment_deadline_on: registered_on,
            ex_rights_on: registered_on,
            settlement_on: registered_on,
            listing_on: registered_on,
            price_per_share,
            mode,
            npc_subscription_strategy:
                crate::company::rights_offering::RightsSubscriptionStrategy::FullByDefault,
        };
        plan.derive_schedule(calendar, exchange, payment_days)
            .map_err(|error| {
                PlayerProposalError::rules(kind, CompanyErrorClass::InvalidInput, error.to_string())
            })?;
        if let Err(error) = self.approve_rights_offering(plan) {
            let class = if self.state.setup.rights_offering_enabled {
                CompanyErrorClass::BusinessCondition
            } else {
                CompanyErrorClass::UnsupportedOperation
            };
            return Err(PlayerProposalError::rules(kind, class, error.to_string()));
        }
        Ok(PlayerProposalReceipt {
            kind,
            identity: event_id,
            approved_on: today,
            announced_on: today,
        })
    }

    /// 按发行人与证券定位完整股东名册（构造分红/送转决议的必要事实）。
    fn registry_for_proposal<'a>(
        &'a self,
        company: &CompanyId,
        stock: &StockCode,
        behavior: &str,
        kind: PlayerProposalKind,
    ) -> Result<&'a crate::company::share_registry::ShareRegistry, PlayerProposalError> {
        self.state
            .corporate_actions
            .registries
            .iter()
            .find(|registry| registry.stock() == stock && registry.issuer() == company)
            .ok_or_else(|| {
                PlayerProposalError::rules(
                    kind,
                    CompanyErrorClass::BusinessCondition,
                    format!("{behavior}需要已显式配置的完整股东名册"),
                )
            })
    }

    /// 提案日程的权益登记日：公告日后首个交易日且不与公告日同日（与偏好自动
    /// 提案同一推导，保证 typed 公告通道照常发布）。
    fn proposal_registered_on(
        &self,
        calendar: &TradingCalendar,
        exchange: crate::calendar::CalendarExchange,
        announced_on: CivilDate,
        kind: PlayerProposalKind,
    ) -> Result<CivilDate, PlayerProposalError> {
        self.proposal_ex_dates(calendar, exchange, announced_on, kind)
            .map(|(registered_on, _)| registered_on)
    }

    /// 提案日程的（登记日, 除息/除权日）对：登记日次一交易日。
    fn proposal_ex_dates(
        &self,
        calendar: &TradingCalendar,
        exchange: crate::calendar::CalendarExchange,
        announced_on: CivilDate,
        kind: PlayerProposalKind,
    ) -> Result<(CivilDate, CivilDate), PlayerProposalError> {
        crate::company::simple::preferences::preference_ex_dates(calendar, exchange, announced_on)
            .map_err(|error| {
                PlayerProposalError::rules(kind, CompanyErrorClass::SystemState, error.to_string())
            })
    }

    fn next_trading_day_for_proposal(
        &self,
        calendar: &TradingCalendar,
        exchange: crate::calendar::CalendarExchange,
        after: CivilDate,
        kind: PlayerProposalKind,
    ) -> Result<CivilDate, PlayerProposalError> {
        calendar.next_trading_day(exchange, after).map_err(|error| {
            PlayerProposalError::rules(kind, CompanyErrorClass::SystemState, error.to_string())
        })
    }

    /// 读取某公司当前的 simple 行为偏好（局内偏好编辑入口的初值；只读投影，
    /// 未知公司显式报错）。
    pub fn company_simple_preferences(
        &self,
        company: &CompanyId,
    ) -> Result<crate::company::simple::preferences::SimpleCompanyPreferences, SessionError> {
        self.state
            .company_system
            .simple_preferences(company)
            .map_err(|error| SessionError::InvalidSetup(error.to_string()))
    }

    /// 偏好局内编辑（N2b，2026-10-08 用户决策「偏好局内编辑即时生效、下周期
    /// 评估」）：任意时刻可改，先完整域校验再写入候选状态；严格持久化由 P 批
    /// 字段既有承载，恢复勾稽要求公司系统配置与新局配置一致，因此两侧同步
    /// 演进（同一候选事务，任一侧失败都不留半配置状态）。修改后同周期内已
    /// 产生的提案是已受理事实，不回滚；下一次结算周期末日评估按新偏好执行。
    pub fn set_simple_preferences(
        &mut self,
        company: &CompanyId,
        preferences: crate::company::simple::preferences::SimpleCompanyPreferences,
    ) -> Result<(), SessionCorporateActionsError> {
        let mut candidate = self.state.company_system.as_ref().clone();
        candidate
            .set_simple_preferences(company, preferences.clone())
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        match &mut self.state.setup.company_system {
            crate::company::config::CompanySystemConfig::Simple(setup_config) => {
                let entry = setup_config
                    .companies
                    .iter_mut()
                    .find(|config| &config.company == company)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(format!(
                            "新局配置缺少公司 {}，偏好编辑无法保持两侧配置一致",
                            company.0
                        ))
                    })?;
                entry.preferences = preferences;
            }
            crate::company::config::CompanySystemConfig::Simulation => {
                return Err(SessionCorporateActionsError::Invalid(
                    "当前公司系统模型非 Simple，不能设置 simple 行为偏好".into(),
                ));
            }
        }
        self.state.company_system = std::sync::Arc::new(candidate);
        Ok(())
    }
}
