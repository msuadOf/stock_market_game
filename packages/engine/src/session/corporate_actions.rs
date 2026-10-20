//! 公司行为 spine 状态容器（B4b 恢复，基 18725d07 `session/corporate_actions.rs`
//! 的**容器子集**）。
//!
//! **本批范围（战役计划 §B4b）**：状态容器形状 + 装配期写入面
//! （`configure_registry` / `configure_cash_dividend_tax_book`）——供
//! `assemble_corporate_foundation` 与 save/restore 使用。
//!
//! **不在本批**（分批边界，见计划 §B4c/B4d/B4e）：
//! - 日终 12 步链（`close_registries_through` / `process_*_on_day_end` /
//!   日终勾稽 `validate`）→ B4c；
//! - 权益计算与 AdminRight 通道、审批/认购命令面 → B4d；
//! - 查询视图族（`dividend_tax_outstanding_views` 等）与 ts_rs 导出面 → B4e/B4f。
//!
//! 因此本文件恢复的结构体不带 `#[ts(export)]`：web 存档 schema
//! （`save/schema/corporate-actions.ts`）随 B4f 的查询面一起接线，届时再
//! 恢复 ts 注解（`SaveSlot` 侧本批按既有公司域权威字段惯例 `#[ts(skip)]`）。

use crate::company::share_registry::{HolderId, ShareRegistry};
use crate::orderbook::AccountId;
use crate::{account::StockCode, money::Money};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// 会话级公司行为权威状态：名册 + 五类公司行为账簿 + 简税回执 + 除权组。
/// `Default` = 未装配任何公司行为事实（公司局构造起点）。
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCorporateActions {
    pub registries: Vec<ShareRegistry>,
    pub dividends: Vec<crate::company::cash_dividend::CashDividendBook>,
    pub dividend_tax_books: Vec<crate::company::cash_dividend_tax::CashDividendTaxBook>,
    /// 简税（FlatWithholding）模式的付款日代扣回执；非 Flat 模式恒为空
    /// （模式门禁勾稽随 B4c 日终链落地）。
    pub flat_withholding_receipts: Vec<FlatWithholdingReceipt>,
    pub stock_distributions: Vec<crate::company::stock_distribution::StockDistributionBook>,
    /// 拆股／缩股（股份重新计值）事件账簿。
    pub share_splits: Vec<crate::company::share_split::ShareSplitBook>,
    pub rights_offerings: Vec<crate::company::rights_offering::RightsOfferingBook>,
    /// 盘中显式认购排队（玩家／宿主当日提交，日终划扣后转入账簿）；
    /// 受理命令通道随 B4d 接线，本批仅随档持久化。
    pub rights_subscription_queue: Vec<QueuedRightsSubscription>,
    /// 日终公开配售超额认购的显式拒绝回执（受理侧额度校验之后的极端竞态兜底）。
    pub rejected_rights_subscriptions: Vec<RejectedRightsSubscription>,
    pub issuer_repurchases: Vec<crate::company::issuer_repurchase::IssuerRepurchaseBook>,
    pub account_gross_receipts: Vec<AccountDividendGrossReceipt>,
    pub external_receipts: Vec<ExternalDividendReceipt>,
    pub applied_ex_reference_groups: Vec<AppliedExReferenceGroup>,
}

/// 盘中排队的显式配股认购（日内提交、当日日终划扣；随日终档持久化）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueuedRightsSubscription {
    pub event_id: String,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub requested_shares: u64,
    pub submitted_on: crate::calendar::CivilDate,
    /// 认购账户（B4a 起 AccountId 全局 canonical 十进制字符串 wire 形态）。
    pub account: AccountId,
}

/// 日终对排队认购的显式拒绝回执：受理侧已按剩余公开额度校验，本回执只兜底
/// 恢复后队列与额度不一致等极端竞态。幂等、可恢复。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectedRightsSubscription {
    pub event_id: String,
    pub account: AccountId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub requested_shares: u64,
    pub submitted_on: crate::calendar::CivilDate,
    /// 拒绝发生的日终日期（≥ 提交日）。
    pub rejected_on: crate::calendar::CivilDate,
    /// 拒绝原因（非空；当前唯一成因是公开配售剩余额度耗尽）。
    pub reason: String,
}

/// 已应用到行情前收锚的除权除息组：同一证券同一除权日只产生一个参考价。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliedExReferenceGroup {
    pub date: crate::calendar::CivilDate,
    pub stock: StockCode,
    pub cash_plan_ids: Vec<String>,
    pub stock_event_ids: Vec<String>,
    pub rights_event_ids: Vec<String>,
    /// 参与合计的拆股／缩股事件。
    pub split_event_ids: Vec<String>,
    pub reference: crate::company::ex_reference_price::ExReferencePrice,
}

/// 简税（FlatWithholding）模式的付款日代扣回执：一位账户持有人一笔付款的
/// 税前应得、代扣比例与实扣金额。代扣与到账同事务原子执行（B4c 日终链语义），
/// 本批随档持久化。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlatWithholdingReceipt {
    pub payment_id: String,
    pub plan_id: String,
    pub account: AccountId,
    pub paid_on: crate::calendar::CivilDate,
    /// 税前应得（分）。
    pub gross: Money,
    /// 开局配置的代扣比例（bp）；随回执冻结，供恢复勾稽复算。
    #[serde(with = "crate::orderbook::js_safe_u64")]
    pub rate_bp: u64,
    /// 实际代扣金额（分）＝ 按比例四舍五入（half-up）到分。
    pub withheld: Money,
}

/// 现金分红付款日单账户的税前应得到账事实（简税/差别化模式共用形态）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountDividendGrossReceipt {
    pub payment_id: String,
    pub plan_id: String,
    pub account: AccountId,
    pub paid_on: crate::calendar::CivilDate,
    pub gross: Money,
    pub tax_status: DividendTaxStatus,
}

/// 外部股东（无游戏账户）的现金分红到账回执事实。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalDividendReceipt {
    pub payment_id: String,
    pub plan_id: String,
    pub holder: crate::company::share_registry::HolderId,
    pub paid_on: crate::calendar::CivilDate,
    pub gross: Money,
    pub tax_status: DividendTaxStatus,
}

/// 分红税务状态（与 18725d07 语义一致；简税模式的判定规则随 B4c 勾稽链接线）。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DividendTaxStatus {
    /// 简税（FlatWithholding）模式：付款日按开局比例对税前应得直接代扣。
    FlatWithholding,
    IndividualPublicMarket,
    TreatmentNotConfigured,
}

/// 税务身份查询的账户身份分类（`taxpayer_identity_of_kind` 的输出）。
///
/// 抽象层留白：非个人身份目前统一为 [`TaxpayerIdentity::NonIndividualPending`]
/// （企业/机构计税未实现）；后续接入企业口径时在此枚举扩展。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TaxpayerIdentity {
    Personal,
    NonIndividualPending,
}

/// 账户种类 → 税务身份：玩家与自然人散户 NPC 是个人；机构/游资/回购专户
/// 是非个人（待企业口径）。装配期自动开账（`AShareIndividual` 模式）只对
/// 个人身份开 `IndividualPublicMarket` 税账。
pub(crate) fn taxpayer_identity_of_kind(kind: crate::account::AccountKind) -> TaxpayerIdentity {
    match kind {
        crate::account::AccountKind::Retail | crate::account::AccountKind::Player => {
            TaxpayerIdentity::Personal
        }
        crate::account::AccountKind::Inst
        | crate::account::AccountKind::Hot
        | crate::account::AccountKind::IssuerRepurchase => TaxpayerIdentity::NonIndividualPending,
    }
}

/// 公司行为状态写入失败的唯一错误形态（装配期与 B4c 日终共用）。
#[derive(Debug, Error)]
pub enum SessionCorporateActionsError {
    #[error("公司行为股东名册配置无效：{0}")]
    Invalid(String),
}

impl SessionCorporateActions {
    /// 装配期写入面：登记一份完整股东名册（勾稽账户持仓与发行人身份后入册）。
    pub(crate) fn configure_registry(
        &mut self,
        registry: ShareRegistry,
        accounts: &BTreeMap<AccountId, BTreeMap<StockCode, u64>>,
        issuers: &crate::company::identity::IssuerRegistry,
    ) -> Result<(), SessionCorporateActionsError> {
        Self::validate_registry(&registry, accounts, issuers)?;
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

    /// 装配期写入面：为名册登记股东开个人股息税账（仅 `AShareIndividual`
    /// 模式的个人身份账户；由 `assemble_corporate_foundation` 调用）。
    pub(crate) fn configure_cash_dividend_tax_book(
        &mut self,
        account: AccountId,
        stock: StockCode,
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
        // 税账以名册日结事实重建 FIFO 批次；事后配置会丢失历史取得/处置事实。
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

    /// 名册勾稽：发行人身份/发行股数与公司身份一致、账户持仓与名册登记一致、
    /// 名册不引用不存在的账户。发行人回购专户的名册身份是 `IssuerTreasury`
    /// （不按 Account 持有人勾稽，一致性由回购批次勾稽承担——B4d 接线）；
    /// 本批装配期专户恒零持仓，与账户面勾稽天然一致。
    fn validate_registry(
        registry: &ShareRegistry,
        accounts: &BTreeMap<AccountId, BTreeMap<StockCode, u64>>,
        issuers: &crate::company::identity::IssuerRegistry,
    ) -> Result<(), SessionCorporateActionsError> {
        registry
            .validate()
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        if issuers.issuer_of(registry.stock()).as_ref() != Some(registry.issuer())
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 容器 serde 契约：默认空容器 roundtrip 逐位；deny_unknown_fields 生效。
    #[test]
    fn default_container_roundtrips_and_rejects_unknown_fields() {
        let container = SessionCorporateActions::default();
        let json = serde_json::to_string(&container).unwrap();
        assert_eq!(
            serde_json::from_str::<SessionCorporateActions>(&json).unwrap(),
            container
        );
        assert!(
            serde_json::from_str::<SessionCorporateActions>(&format!(
                "{{\"registries\": [], \"dividends\": [], \"dividend_tax_books\": [], \
                 \"flat_withholding_receipts\": [], \"stock_distributions\": [], \
                 \"share_splits\": [], \"rights_offerings\": [], \
                 \"rights_subscription_queue\": [], \"rejected_rights_subscriptions\": [], \
                 \"issuer_repurchases\": [], \"account_gross_receipts\": [], \
                 \"external_receipts\": [], \"applied_ex_reference_groups\": [], \
                 \"unknown_field\": []}}"
            ))
            .is_err(),
            "未知字段必须显式拒绝（deny_unknown_fields）"
        );
    }
}
