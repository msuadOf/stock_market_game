use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{account::StockCode, calendar::CivilDate, money::Money, orderbook::AccountId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DividendTaxProfile {
    IndividualPublicMarket,
    ResidentEnterprise,
    SecuritiesFund,
    NonResident,
}

/// 新局现金分红税务模式（开局产品选项，2026-10-08 三层决策登记）。
///
/// - 默认 [`CashDividendTaxMode::FlatWithholding`]（简税）：分红付款日对名册每位
///   账户持有人按 `SessionSetup::flat_withholding_bp` 比例对税前应得直接代扣；
///   无持股期档位、无税账 FIFO、机构持有人同样代扣（机构不另算），卖出不补税。
///   不创建、不持久化任何 `CashDividendTaxBook`。
/// - [`CashDividendTaxMode::AShareIndividual`]（大 A 方式）：现行个人公开市场
///   差别化口径的全部行为——装配期配置股东名册时为每个「个人」身份账户（玩家与
///   自然人散户 NPC）自动配置 `IndividualPublicMarket` 税账，按三档税率、FIFO
///   持股期限、转让时扣收与部分收缴追缴运行；机构/游资等非个人身份保持
///   `TreatmentNotConfigured`（企业/机构计税未实现，见 `TaxpayerIdentity` 的扩展位）。
/// - [`CashDividendTaxMode::Exempt`]（不扣税）：不产生个人股息税事实，且交易环节
///   卖出印花税免征（`SessionSetup` 校验强制该模式 `stamp_tax_rate == 0`，
///   佣金与过户费照付）；引擎装配期不为任何身份配置税账（宿主仍可用显式装配期
///   命令配置，既有入口语义不变）。
///
/// 该模式作为严格持久化状态随 `SessionSetup` 进存档契约：新档必填、无 serde 默认，
/// 缺失该字段的旧档在反序列化时被显式拒绝；旧两变体枚举值（`IndividualPublicMarket`）
/// 已按无兼容原则整体删除，携带旧值的档显式拒绝，不静默映射。恢复后语义不变。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum CashDividendTaxMode {
    FlatWithholding,
    AShareIndividual,
    Exempt,
}

/// 简税比例（basis points）的合法上限：100%（10000bp）。超过全额代扣会破坏
/// 「代扣额 ≤ 税前应得 ≤ 到账后现金」的原子代扣不变量，故显式拒绝。
pub const MAX_FLAT_WITHHOLDING_BP: u32 = 10_000;

/// Flat 模式下按持有人税前应得计算代扣额：四舍五入（half-up）到整数分，
/// 与个人差别化口径的「每笔分红合计应纳税额四舍五入到分」实现口径一致。
pub fn flat_withholding_cents(
    gross_cents: i64,
    rate_bp: u32,
) -> Result<i64, DividendTaxError> {
    let scaled = i128::from(gross_cents)
        .checked_mul(i128::from(rate_bp))
        .and_then(|value| value.checked_add(5_000))
        .ok_or_else(|| overflow("flat withholding multiplication"))?;
    let cents = scaled / 10_000;
    if cents < 0 || cents > i128::from(gross_cents.max(0)) {
        return Err(invalid(
            "flat withholding must stay within the nonnegative gross entitlement",
        ));
    }
    i64::try_from(cents).map_err(|_| overflow("flat withholding cents"))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum StatutoryRestrictedBasis {
    FinanceTax2009167,
    FinanceTax201070,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum TaxShareClass {
    PublicMarket,
    StatutoryRestricted {
        release_on: CivilDate,
        basis: StatutoryRestrictedBasis,
        qualification_evidence: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum TaxAcquisitionSource {
    InitialAllocation { evidence: String },
    SecondaryMarket { settlement: String },
    CorporateAction { event: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DividendTaxLot {
    pub id: String,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub qty: u64,
    pub acquired_on: CivilDate,
    pub source: TaxAcquisitionSource,
    pub class: TaxShareClass,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ExactAmountState")]
pub struct ExactDividendTaxAmount {
    #[serde(with = "nonnegative_decimal")]
    numerator: i128,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    denominator: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactAmountState {
    #[serde(with = "nonnegative_decimal")]
    numerator: i128,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    denominator: u64,
}

impl TryFrom<ExactAmountState> for ExactDividendTaxAmount {
    type Error = DividendTaxError;
    fn try_from(state: ExactAmountState) -> Result<Self, Self::Error> {
        let amount = Self::new(state.numerator, state.denominator)?;
        if amount.numerator != state.numerator || amount.denominator != state.denominator {
            return Err(invalid(
                "exact amount must be a reduced nonnegative fraction",
            ));
        }
        Ok(amount)
    }
}

impl ExactDividendTaxAmount {
    pub fn new(numerator: i128, denominator: u64) -> Result<Self, DividendTaxError> {
        if numerator < 0 || denominator == 0 {
            return Err(invalid("invalid exact tax amount"));
        }
        let divisor = gcd(numerator as u128, u128::from(denominator));
        Ok(Self {
            numerator: numerator / divisor as i128,
            denominator: denominator / divisor as u64,
        })
    }
    pub fn numerator(&self) -> i128 {
        self.numerator
    }
    pub fn denominator(&self) -> u64 {
        self.denominator
    }
    fn zero() -> Self {
        Self {
            numerator: 0,
            denominator: 1,
        }
    }
    fn add(&self, other: &Self) -> Result<Self, DividendTaxError> {
        let divisor = gcd(u128::from(self.denominator), u128::from(other.denominator)) as u64;
        let left_factor = other.denominator / divisor;
        let right_factor = self.denominator / divisor;
        let numerator = self
            .numerator
            .checked_mul(i128::from(left_factor))
            .and_then(|left| {
                other
                    .numerator
                    .checked_mul(i128::from(right_factor))
                    .and_then(|right| left.checked_add(right))
            })
            .ok_or_else(|| overflow("exact amount addition"))?;
        let denominator = self
            .denominator
            .checked_mul(left_factor)
            .ok_or_else(|| overflow("exact amount denominator"))?;
        Self::new(numerator, denominator)
    }
    fn subtract_cents(&self, cents: i128) -> Result<Self, DividendTaxError> {
        let numerator = self
            .numerator
            .checked_sub(
                cents
                    .checked_mul(i128::from(self.denominator))
                    .ok_or_else(|| overflow("paid tax scaling"))?,
            )
            .ok_or_else(|| overflow("paid tax subtraction"))?;
        Self::new(numerator, self.denominator)
    }
    fn multiply(&self, quantity: u64, percent: u8) -> Result<Self, DividendTaxError> {
        let mut numerator = self.numerator as u128;
        let mut quantity = u128::from(quantity);
        let mut percent = u128::from(percent);
        let mut denominator = u128::from(self.denominator);
        let mut hundred = 100u128;
        for factor in [&mut numerator, &mut quantity, &mut percent] {
            let divisor = gcd(*factor, denominator);
            *factor /= divisor;
            denominator /= divisor;
            let divisor = gcd(*factor, hundred);
            *factor /= divisor;
            hundred /= divisor;
        }
        let numerator = numerator
            .checked_mul(quantity)
            .and_then(|value| value.checked_mul(percent))
            .and_then(|value| i128::try_from(value).ok())
            .ok_or_else(|| overflow("exact tax multiplication"))?;
        let denominator = denominator
            .checked_mul(hundred)
            .and_then(|value| u64::try_from(value).ok())
            .ok_or_else(|| overflow("exact tax denominator"))?;
        Self::new(numerator, denominator)
    }
    /// 四舍五入（half-up）到整数分。收缴边界按“持有人每笔分红合计应纳税额
    /// 四舍五入到分”的登记口径取整；登记与评估事实仍保留精确分数。
    fn round_half_up_cents(&self) -> Result<i128, DividendTaxError> {
        let numerator = u128::try_from(self.numerator).map_err(|_| overflow("tax rounding"))?;
        let denominator = u128::from(self.denominator);
        let doubled_denominator = denominator
            .checked_mul(2)
            .ok_or_else(|| overflow("tax rounding denominator"))?;
        let scaled = numerator
            .checked_mul(2)
            .and_then(|value| value.checked_add(denominator))
            .ok_or_else(|| overflow("tax rounding numerator"))?;
        i128::try_from(scaled / doubled_denominator).map_err(|_| overflow("tax rounding cents"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum DividendTaxError {
    #[error("cash dividend tax: {0}")]
    InvalidFact(String),
    #[error("cash dividend tax: overflow in {operation}")]
    Overflow { operation: &'static str },
    #[error("cash dividend tax: unsupported taxpayer profile {profile:?}")]
    UnsupportedProfile { profile: DividendTaxProfile },
    #[error("cash dividend tax: mixed restricted tax lots require verified classification rules")]
    UnsupportedMixedRestrictedTaxLots,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaxDisposition {
    lot: DividendTaxLot,
    disposed_on: CivilDate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxDayReceipt {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    pub(crate) operation_seq: u64,
    pub(crate) event_id: String,
    pub(crate) day: CivilDate,
    #[serde(with = "crate::company::share_registry::canonical_i128_decimal")]
    pub(crate) net_change: i128,
    #[serde(deserialize_with = "required_lot")]
    pub(crate) acquisition: Option<DividendTaxLot>,
    pub(crate) dispositions: Vec<TaxDisposition>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DividendCashReceipt {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    operation_seq: u64,
    payment_id: String,
    paid_on: CivilDate,
    received_gross: Money,
    evidence: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisteredTaxDividend {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    operation_seq: u64,
    event_id: String,
    registered_on: CivilDate,
    per_share: ExactDividendTaxAmount,
    lots: Vec<DividendTaxLot>,
    payments: Vec<DividendCashReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxCollectionReceipt {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    pub operation_seq: u64,
    pub event_id: String,
    pub day: CivilDate,
    pub available_cash: Money,
    pub collected: Money,
    pub remaining_cash: Money,
    pub outstanding: ExactDividendTaxAmount,
    pub needs_funds: bool,
}

/// 拆股／缩股的税账净核减回执（缩股方向专用）。
///
/// 缩股是股份重新计值，不是转让：核减按 FIFO 减少 lot 数量并留痕核减片段，
/// 但**不进入应税处置口径**（`assessed_cents_through` 只统计公开市场处置）；
/// 持股期限的取得日由存活 lot 原样延续（无官方明文，登记为游戏实现口径）。
/// 拆股方向的新增股份走既有 `record_net_day` 同日正向续记（取得日 = R+1，
/// 与送转同一保守口径）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxRedenominationReceipt {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    pub(crate) operation_seq: u64,
    pub(crate) event_id: String,
    pub(crate) day: CivilDate,
    #[serde(with = "crate::company::share_registry::canonical_i128_decimal")]
    pub(crate) net_change: i128,
    /// FIFO 核减的 lot 片段（事实留痕；限售类 lot 允许被核减——不是处置）。
    pub(crate) removed: Vec<TaxDisposition>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TaxBookState")]
pub struct CashDividendTaxBook {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    operation_seq: u64,
    account: AccountId,
    stock: StockCode,
    profile: DividendTaxProfile,
    opened_on: CivilDate,
    opening_lots: Vec<DividendTaxLot>,
    settled_on: CivilDate,
    lots: Vec<DividendTaxLot>,
    days: Vec<TaxDayReceipt>,
    dividends: Vec<RegisteredTaxDividend>,
    collections: Vec<TaxCollectionReceipt>,
    redenominations: Vec<TaxRedenominationReceipt>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaxBookState {
    operation_seq: u64,
    account: AccountId,
    stock: StockCode,
    profile: DividendTaxProfile,
    opened_on: CivilDate,
    opening_lots: Vec<DividendTaxLot>,
    settled_on: CivilDate,
    lots: Vec<DividendTaxLot>,
    days: Vec<TaxDayReceipt>,
    dividends: Vec<RegisteredTaxDividend>,
    collections: Vec<TaxCollectionReceipt>,
    redenominations: Vec<TaxRedenominationReceipt>,
}

impl TryFrom<TaxBookState> for CashDividendTaxBook {
    type Error = DividendTaxError;
    fn try_from(state: TaxBookState) -> Result<Self, Self::Error> {
        let book = Self {
            operation_seq: state.operation_seq,
            account: state.account,
            stock: state.stock,
            profile: state.profile,
            opened_on: state.opened_on,
            opening_lots: state.opening_lots,
            settled_on: state.settled_on,
            lots: state.lots,
            days: state.days,
            dividends: state.dividends,
            collections: state.collections,
            redenominations: state.redenominations,
        };
        book.validate()?;
        Ok(book)
    }
}

impl CashDividendTaxBook {
    pub fn new(
        account: AccountId,
        stock: StockCode,
        profile: DividendTaxProfile,
        settled_on: CivilDate,
        lots: Vec<DividendTaxLot>,
    ) -> Result<Self, DividendTaxError> {
        let book = Self {
            operation_seq: 0,
            account,
            stock,
            profile,
            opened_on: settled_on,
            opening_lots: lots.clone(),
            settled_on,
            lots,
            days: vec![],
            dividends: vec![],
            collections: vec![],
            redenominations: vec![],
        };
        book.validate()?;
        Ok(book)
    }
    pub fn register_dividend(
        &mut self,
        event_id: String,
        registered_on: CivilDate,
        per_share: ExactDividendTaxAmount,
    ) -> Result<(), DividendTaxError> {
        if let Some(existing) = self
            .dividends
            .iter()
            .find(|event| event.event_id == event_id)
        {
            if existing.registered_on == registered_on && existing.per_share == per_share {
                return Ok(());
            }
            return Err(invalid(
                "dividend identity reused with different registration",
            ));
        }
        if event_id.trim().is_empty()
            || registered_on != self.settled_on
            || per_share.numerator == 0
            || self.lots.is_empty()
        {
            return Err(invalid(
                "dividend registration needs current completed day, held shares and positive rate",
            ));
        }
        let mut candidate = self.clone();
        let operation_seq = candidate.advance_operation()?;
        candidate.dividends.push(RegisteredTaxDividend {
            operation_seq,
            event_id,
            registered_on,
            per_share,
            lots: self.lots.clone(),
            payments: vec![],
        });
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub fn record_net_day(
        &mut self,
        event_id: String,
        day: CivilDate,
        net_change: i128,
        acquisition: Option<DividendTaxLot>,
    ) -> Result<(), DividendTaxError> {
        if let Some(existing) = self
            .days
            .iter()
            .find(|receipt| receipt.event_id == event_id)
        {
            if existing.day == day
                && existing.net_change == net_change
                && existing.acquisition == acquisition
            {
                return Ok(());
            }
            return Err(invalid("tax day identity reused with different facts"));
        }
        if event_id.trim().is_empty() {
            return Err(invalid("tax day identity must be a non-empty string"));
        }
        // 同日续记：R+1 到账的送转新股在当日公开市场日结之后追加一条正向
        // 净增（财税〔2012〕85号第三条的每日期末净增事实），事件身份独立于
        // 公开市场日结；处置仍每日只按公开市场净额发生一次，故续记必须为正。
        let day_gap = day.days_since(self.settled_on);
        let same_day_continuation = !self.days.is_empty() && day_gap == 0 && net_change > 0;
        if !same_day_continuation && day_gap != 1 {
            return Err(invalid(
                "tax day must follow previous completed natural day or continue it with a positive corporate-action credit",
            ));
        }
        if (net_change > 0) != acquisition.is_some() {
            return Err(invalid(
                "only positive daily net acquisition has a new tax lot",
            ));
        }
        let mut candidate = self.clone();
        if let Some(lot) = &acquisition {
            validate_lot(lot, day)?;
            if lot.acquired_on != day
                || i128::from(lot.qty) != net_change
                || candidate.has_lot_id(&lot.id)
            {
                return Err(invalid(
                    "new tax lot date, quantity or identity conflicts with daily net acquisition",
                ));
            }
        }
        let dispositions =
            apply_net_change(&mut candidate.lots, day, net_change, acquisition.as_ref())?;
        let operation_seq = candidate.advance_operation()?;
        candidate.days.push(TaxDayReceipt {
            operation_seq,
            event_id,
            day,
            net_change,
            acquisition,
            dispositions,
        });
        candidate.settled_on = day;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// 拆股／缩股的税账净核减（缩股方向）：FIFO 减少 lot 数量并留痕，不进入
    /// 应税处置口径。同日续记（公开市场日结之后追加）或次一自然日均可；
    /// 同一事件身份幂等、不同事实显式冲突。
    pub fn record_redenomination_reduction(
        &mut self,
        event_id: String,
        day: CivilDate,
        removed_qty: u64,
    ) -> Result<(), DividendTaxError> {
        if let Some(existing) = self
            .redenominations
            .iter()
            .find(|receipt| receipt.event_id == event_id)
        {
            if existing.day == day && existing.net_change == -i128::from(removed_qty) {
                return Ok(());
            }
            return Err(invalid(
                "redenomination identity reused with different facts",
            ));
        }
        if event_id.trim().is_empty() || removed_qty == 0 {
            return Err(invalid(
                "redenomination requires identity and a positive share reduction",
            ));
        }
        let day_gap = day.days_since(self.settled_on);
        if day_gap != 0 && day_gap != 1 {
            return Err(invalid(
                "redenomination must follow the previous completed natural day or continue it",
            ));
        }
        let mut candidate = self.clone();
        let removed = apply_redenomination_reduction(&mut candidate.lots, day, removed_qty)?;
        let operation_seq = candidate.advance_operation()?;
        candidate.redenominations.push(TaxRedenominationReceipt {
            operation_seq,
            event_id,
            day,
            net_change: -i128::from(removed_qty),
            removed,
        });
        candidate.settled_on = day;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    pub fn record_payment(
        &mut self,
        event_id: &str,
        payment_id: String,
        paid_on: CivilDate,
        received_gross: Money,
        evidence: String,
    ) -> Result<(), DividendTaxError> {
        if let Some((dividend, existing)) = self.dividends.iter().find_map(|dividend| {
            dividend
                .payments
                .iter()
                .find(|existing| existing.payment_id == payment_id)
                .map(|existing| (dividend, existing))
        }) {
            if dividend.event_id == event_id
                && existing.paid_on == paid_on
                && existing.received_gross == received_gross
                && existing.evidence == evidence
            {
                return Ok(());
            }
            return Err(invalid(
                "payment identity reused with different actual receipt",
            ));
        }
        if paid_on != self.settled_on
            || received_gross.cents() <= 0
            || payment_id.trim().is_empty()
            || evidence.trim().is_empty()
        {
            return Err(invalid(
                "payment requires current completed date, positive real cash and receipt evidence",
            ));
        }
        let mut candidate = self.clone();
        let operation_seq = candidate.advance_operation()?;
        let receipt = DividendCashReceipt {
            operation_seq,
            payment_id,
            paid_on,
            received_gross,
            evidence,
        };
        let dividend = candidate
            .dividends
            .iter_mut()
            .find(|dividend| dividend.event_id == event_id)
            .ok_or_else(|| invalid("unknown registered dividend"))?;
        dividend.payments.push(receipt);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
    /// 当前未划收税额（分）。每笔分红按持有人合计应纳税额先精确求值、再按登记口径
    /// 四舍五入到整数分，最后扣除已划收金额；结果恒为整数分。
    pub fn outstanding(&self) -> Result<ExactDividendTaxAmount, DividendTaxError> {
        let collected = self.collections.iter().try_fold(0i128, |total, receipt| {
            total
                .checked_add(i128::from(receipt.collected.cents()))
                .ok_or_else(|| overflow("collected tax sum"))
        })?;
        let cents = self
            .assessed_cents_through(self.settled_on, self.operation_seq)?
            .checked_sub(collected)
            .ok_or_else(|| overflow("outstanding tax subtraction"))?;
        // 负值说明已划收金额超过当前评估税额，属不变量破坏；new 会显式拒绝而非静默钳位。
        ExactDividendTaxAmount::new(cents, 1)
    }
    pub fn collect_due(
        &mut self,
        event_id: String,
        day: CivilDate,
        available_cash: Money,
    ) -> Result<TaxCollectionReceipt, DividendTaxError> {
        if let Some(existing) = self
            .collections
            .iter()
            .find(|receipt| receipt.event_id == event_id)
        {
            if existing.day == day && existing.available_cash == available_cash {
                return Ok(existing.clone());
            }
            return Err(invalid(
                "tax collection identity reused with different available cash",
            ));
        }
        if event_id.trim().is_empty() || day != self.settled_on || available_cash.cents() < 0 {
            return Err(invalid(
                "collection requires current date and nonnegative real available cash",
            ));
        }
        let due = self.outstanding()?;
        let cents = i64::try_from(due.numerator.min(i128::from(available_cash.cents())))
            .map_err(|_| overflow("tax collection cash"))?;
        let receipt = TaxCollectionReceipt {
            operation_seq: self
                .operation_seq
                .checked_add(1)
                .ok_or_else(|| overflow("operation sequence"))?,
            event_id,
            day,
            available_cash,
            collected: Money::from_cents(cents),
            remaining_cash: Money::from_cents(available_cash.cents() - cents),
            outstanding: due.subtract_cents(i128::from(cents))?,
            needs_funds: due.numerator > i128::from(cents),
        };
        let mut candidate = self.clone();
        candidate.advance_operation()?;
        candidate.collections.push(receipt.clone());
        candidate.validate()?;
        *self = candidate;
        Ok(receipt)
    }
    pub fn lots(&self) -> &[DividendTaxLot] {
        &self.lots
    }
    pub fn outstanding_tax(&self) -> Result<ExactDividendTaxAmount, DividendTaxError> {
        self.outstanding()
    }
    pub fn collections(&self) -> &[TaxCollectionReceipt] {
        &self.collections
    }
    /// 拆股／缩股核减回执按事件身份查询（与日结回执同一幂等判定入口）。
    pub(crate) fn redenomination_by_event(
        &self,
        event_id: &str,
    ) -> Option<&TaxRedenominationReceipt> {
        self.redenominations
            .iter()
            .find(|receipt| receipt.event_id == event_id)
    }

    pub(crate) fn receipt_by_event(&self, event_id: &str) -> Option<&TaxDayReceipt> {
        self.days
            .iter()
            .find(|receipt| receipt.event_id == event_id)
    }
    /// 日结回执只读视图（含同日续记的完整顺序），供 Session 层勾稽与测试断言使用。
    pub(crate) fn tax_day_receipts(&self) -> &[TaxDayReceipt] {
        &self.days
    }
    pub(crate) fn dividend_event_by_id(&self, event_id: &str) -> Option<&RegisteredTaxDividend> {
        self.dividends
            .iter()
            .find(|dividend| dividend.event_id == event_id)
    }
    pub(crate) fn payment_by_id(&self, payment_id: &str) -> Option<&DividendCashReceipt> {
        self.dividends
            .iter()
            .flat_map(|dividend| &dividend.payments)
            .find(|payment| payment.payment_id == payment_id)
    }
    /// 指定自然日是否有新到账的税前分红（用于日终决定是否留税额回执）。
    pub(crate) fn has_payment_on(&self, day: CivilDate) -> bool {
        self.dividends.iter().any(|dividend| {
            dividend
                .payments
                .iter()
                .any(|payment| payment.paid_on == day)
        })
    }
    /// 指定自然日是否发生 FIFO 处置（用于日终决定是否留税额回执）。
    pub(crate) fn has_disposition_on(&self, day: CivilDate) -> bool {
        self.days
            .iter()
            .any(|receipt| receipt.day == day && !receipt.dispositions.is_empty())
    }
    pub fn account(&self) -> AccountId {
        self.account
    }
    pub fn stock(&self) -> &StockCode {
        &self.stock
    }
    pub fn settled_on(&self) -> CivilDate {
        self.settled_on
    }
    fn has_lot_id(&self, id: &str) -> bool {
        self.lots.iter().any(|lot| lot.id == id)
            || self.days.iter().any(|day| {
                day.acquisition.as_ref().is_some_and(|lot| lot.id == id)
                    || day
                        .dispositions
                        .iter()
                        .any(|disposed| disposed.lot.id == id)
            })
            || self
                .dividends
                .iter()
                .any(|event| event.lots.iter().any(|lot| lot.id == id))
    }
    /// 截至指定日期与操作序号、按“每笔分红合计应纳税额四舍五入到分”口径
    /// 计算的整分应纳税额。每笔分红内部仍用精确分数求值，仅在汇总后取整。
    fn assessed_cents_through(
        &self,
        through: CivilDate,
        operation_seq: u64,
    ) -> Result<i128, DividendTaxError> {
        let mut total_cents = 0i128;
        for dividend in self
            .dividends
            .iter()
            .filter(|dividend| dividend.operation_seq <= operation_seq)
        {
            let quantity = total_quantity(&dividend.lots)?;
            let mut assessed = ExactDividendTaxAmount::zero();
            for payment in dividend.payments.iter().filter(|payment| {
                payment.paid_on <= through && payment.operation_seq <= operation_seq
            }) {
                let per_share = ExactDividendTaxAmount::new(
                    i128::from(payment.received_gross.cents()),
                    quantity,
                )?;
                for lot in &dividend.lots {
                    if matches!(&lot.class, TaxShareClass::StatutoryRestricted { release_on, .. } if payment.paid_on < *release_on)
                    {
                        assessed = assessed.add(&per_share.multiply(lot.qty, 10)?)?;
                        continue;
                    }
                    let start = match &lot.class {
                        TaxShareClass::PublicMarket => lot.acquired_on,
                        TaxShareClass::StatutoryRestricted { release_on, .. } => *release_on,
                    };
                    for day in self.days.iter().filter(|day| {
                        day.day > dividend.registered_on
                            && day.day <= through
                            && day.operation_seq <= operation_seq
                    }) {
                        for disposition in day
                            .dispositions
                            .iter()
                            .filter(|disposition| disposition.lot.id == lot.id)
                        {
                            assessed = assessed.add(&per_share.multiply(
                                disposition.lot.qty,
                                personal_cash_dividend_rate(start, disposition.disposed_on)?,
                            )?)?;
                        }
                    }
                }
            }
            total_cents = total_cents
                .checked_add(assessed.round_half_up_cents()?)
                .ok_or_else(|| overflow("assessed tax cents"))?;
        }
        Ok(total_cents)
    }
    pub fn validate(&self) -> Result<(), DividendTaxError> {
        let mut operations = Vec::new();
        for day in &self.days {
            operations.push((day.operation_seq, day.day));
        }
        for dividend in &self.dividends {
            operations.push((dividend.operation_seq, dividend.registered_on));
            for payment in &dividend.payments {
                if payment.operation_seq <= dividend.operation_seq {
                    return Err(invalid("payment precedes dividend registration"));
                }
                operations.push((payment.operation_seq, payment.paid_on));
            }
        }
        for collection in &self.collections {
            operations.push((collection.operation_seq, collection.day));
        }
        for reduction in &self.redenominations {
            operations.push((reduction.operation_seq, reduction.day));
        }
        operations.sort_unstable_by_key(|(sequence, _)| *sequence);
        let mut previous_fact_day = self.opened_on;
        for (index, (sequence, day)) in operations.iter().enumerate() {
            let expected = u64::try_from(index)
                .ok()
                .and_then(|index| index.checked_add(1))
                .ok_or_else(|| overflow("operation sequence"))?;
            if *sequence != expected || *day < previous_fact_day || *day > self.settled_on {
                return Err(invalid(
                    "operation facts must have consecutive unique sequence and nondecreasing dates",
                ));
            }
            previous_fact_day = *day;
        }
        if u64::try_from(operations.len()).map_err(|_| overflow("operation sequence"))?
            != self.operation_seq
        {
            return Err(invalid(
                "operation sequence does not match actual fact tail",
            ));
        }
        if self.profile != DividendTaxProfile::IndividualPublicMarket {
            return Err(DividendTaxError::UnsupportedProfile {
                profile: self.profile.clone(),
            });
        }
        if self.stock.0.trim().is_empty() || self.settled_on < self.opened_on {
            return Err(invalid("invalid tax book stock or opening/settled date"));
        }
        validate_lots(&self.lots, self.settled_on)?;
        validate_lots(&self.opening_lots, self.opened_on)?;
        let mut replayed = self.opening_lots.clone();
        let mut seen_lot_ids: BTreeSet<String> =
            replayed.iter().map(|lot| lot.id.clone()).collect();
        for (sequence, _) in &operations {
            if let Some(day) = self.days.iter().find(|day| day.operation_seq == *sequence) {
                if let Some(lot) = &day.acquisition {
                    if !seen_lot_ids.insert(lot.id.clone()) {
                        return Err(invalid("historical tax lot identity reused"));
                    }
                }
                let dispositions = apply_net_change(
                    &mut replayed,
                    day.day,
                    day.net_change,
                    day.acquisition.as_ref(),
                )?;
                if dispositions != day.dispositions {
                    return Err(invalid("tax disposal identities, metadata or FIFO quantities contradict held facts"));
                }
            }
            if let Some(reduction) = self
                .redenominations
                .iter()
                .find(|receipt| receipt.operation_seq == *sequence)
            {
                let removed =
                    apply_redenomination_reduction(&mut replayed, reduction.day, {
                        u64::try_from(
                            reduction
                                .net_change
                                .checked_neg()
                                .ok_or_else(|| overflow("negative redenomination shares"))?,
                        )
                        .map_err(|_| invalid("redenomination reduction exceeds u64"))?
                    })?;
                if removed != reduction.removed {
                    return Err(invalid(
                        "redenomination removed fragments contradict FIFO replay",
                    ));
                }
            }
            if let Some(dividend) = self
                .dividends
                .iter()
                .find(|dividend| dividend.operation_seq == *sequence)
            {
                if dividend.lots != replayed {
                    return Err(invalid("registered dividend tax lots contradict actual holding facts at registration"));
                }
            }
        }
        if replayed != self.lots {
            return Err(invalid("current tax lots contradict completed net changes"));
        }
        let mut day_ids = BTreeSet::new();
        let mut acquired_ids = BTreeSet::new();
        let mut previous_day = self.opened_on;
        let mut seen_any_day = false;
        for day in &self.days {
            // 首条日结必须紧邻开账日的下一自然日；其后允许「同日正向续记」
            // （R+1 送转到账在公开市场日结之后追加），除此之外仍须逐日连续。
            let day_gap = day.day.days_since(previous_day);
            let same_day_continuation = seen_any_day && day_gap == 0 && day.net_change > 0;
            if day.event_id.trim().is_empty()
                || !day_ids.insert(&day.event_id)
                || (!same_day_continuation && day_gap != 1)
                || day.day > self.settled_on
            {
                return Err(invalid(
                    "tax day receipts need unique identities and consecutive dates",
                ));
            }
            seen_any_day = true;
            previous_day = day.day;
            if (day.net_change > 0) != day.acquisition.is_some() {
                return Err(invalid(
                    "tax day acquisition does not match positive net change",
                ));
            }
            if let Some(lot) = &day.acquisition {
                validate_lot(lot, day.day)?;
                if lot.acquired_on != day.day
                    || i128::from(lot.qty) != day.net_change
                    || !acquired_ids.insert(&lot.id)
                {
                    return Err(invalid("tax day acquisition facts are inconsistent"));
                }
            }
            let mut disposed_ids = BTreeSet::new();
            let mut total = 0i128;
            let mut last_acquired = None;
            for disposed in &day.dispositions {
                validate_lot(&disposed.lot, day.day)?;
                if disposed.disposed_on != day.day
                    || !disposed_ids.insert(&disposed.lot.id)
                    || last_acquired.is_some_and(|previous| previous > disposed.lot.acquired_on)
                    || matches!(&disposed.lot.class, TaxShareClass::StatutoryRestricted { release_on, .. } if day.day < *release_on)
                {
                    return Err(invalid("invalid tax FIFO disposal facts"));
                }
                last_acquired = Some(disposed.lot.acquired_on);
                total = total
                    .checked_add(i128::from(disposed.lot.qty))
                    .ok_or_else(|| overflow("tax disposition quantity"))?;
            }
            let expected = if day.net_change < 0 {
                day.net_change
                    .checked_neg()
                    .ok_or_else(|| overflow("negative net shares"))?
            } else {
                0
            };
            if total != expected {
                return Err(invalid("tax dispositions do not match day net reduction"));
            }
        }
        // 拆股／缩股核减回执：身份唯一、净负、片段合计与净额一致、FIFO 顺序；
        // 限售片段允许出现（不是处置）。日期次序由上方合并操作序的全局
        // 非递减校验承载（核减可插在历史中段，恢复重放按操作序进行）。
        let mut redenomination_ids = BTreeSet::new();
        for reduction in &self.redenominations {
            if reduction.event_id.trim().is_empty()
                || !redenomination_ids.insert(&reduction.event_id)
                || reduction.net_change >= 0
                || reduction.day > self.settled_on
            {
                return Err(invalid(
                    "invalid tax re-denomination receipt identity, sign, or date",
                ));
            }
            let mut removed_ids = BTreeSet::new();
            let mut total = 0i128;
            let mut last_acquired = None;
            for removed in &reduction.removed {
                validate_lot(&removed.lot, reduction.day)?;
                if removed.disposed_on != reduction.day
                    || !removed_ids.insert(&removed.lot.id)
                    || last_acquired.is_some_and(|previous| previous > removed.lot.acquired_on)
                {
                    return Err(invalid("invalid tax re-denomination removal facts"));
                }
                last_acquired = Some(removed.lot.acquired_on);
                total = total
                    .checked_add(i128::from(removed.lot.qty))
                    .ok_or_else(|| overflow("tax re-denomination quantity"))?;
            }
            let expected = reduction
                .net_change
                .checked_neg()
                .ok_or_else(|| overflow("negative redenomination shares"))?;
            if total != expected {
                return Err(invalid(
                    "tax re-denomination removals do not match the net reduction",
                ));
            }
        }
        // 收尾日须等于最后一条事实（日结或重新计值核减）的日期。
        let tail_day = previous_day
            .max(self.redenominations.last().map(|receipt| receipt.day).unwrap_or(previous_day));
        if tail_day != self.settled_on {
            return Err(invalid("tax settled day does not match receipt tail"));
        }
        let mut dividend_ids = BTreeSet::new();
        let mut payment_ids = BTreeSet::new();
        for dividend in &self.dividends {
            if dividend.event_id.trim().is_empty()
                || !dividend_ids.insert(&dividend.event_id)
                || dividend.registered_on < self.opened_on
                || dividend.registered_on > self.settled_on
                || dividend.per_share.numerator == 0
            {
                return Err(invalid("invalid immutable dividend registration"));
            }
            validate_lots(&dividend.lots, dividend.registered_on)?;
            let quantity = total_quantity(&dividend.lots)?;
            if quantity == 0 {
                return Err(invalid("registered dividend has no entitled shares"));
            }
            let mut received = 0i128;
            for payment in &dividend.payments {
                if payment.payment_id.trim().is_empty()
                    || !payment_ids.insert(&payment.payment_id)
                    || payment.evidence.trim().is_empty()
                    || payment.received_gross.cents() <= 0
                    || payment.paid_on < dividend.registered_on
                    || payment.paid_on > self.settled_on
                {
                    return Err(invalid("invalid actual dividend cash receipt"));
                }
                received = received
                    .checked_add(i128::from(payment.received_gross.cents()))
                    .ok_or_else(|| overflow("received dividends"))?;
            }
            let paid_scaled = received
                .checked_mul(i128::from(dividend.per_share.denominator))
                .ok_or_else(|| overflow("received dividend comparison"))?;
            let declared_scaled = dividend
                .per_share
                .numerator
                .checked_mul(i128::from(quantity))
                .ok_or_else(|| overflow("declared dividend comparison"))?;
            if paid_scaled > declared_scaled {
                return Err(invalid(
                    "received cash exceeds registered gross entitlement",
                ));
            }
            for lot in &dividend.lots {
                let disposed = self
                    .days
                    .iter()
                    .filter(|day| day.day > dividend.registered_on)
                    .flat_map(|day| &day.dispositions)
                    .filter(|disposed| disposed.lot.id == lot.id)
                    .try_fold(0u64, |total, disposed| {
                        total
                            .checked_add(disposed.lot.qty)
                            .ok_or_else(|| overflow("registered disposed quantity"))
                    })?;
                if disposed > lot.qty {
                    return Err(invalid("a dividend entitlement is disposed more than once"));
                }
            }
        }
        let mut collection_ids = BTreeSet::new();
        let mut collected = 0i128;
        let mut last_collection_day = self.opened_on;
        for collection in &self.collections {
            if collection.event_id.trim().is_empty()
                || !collection_ids.insert(&collection.event_id)
                || collection.day < last_collection_day
                || collection.day > self.settled_on
                || collection.available_cash.cents() < 0
                || collection.collected.cents() < 0
            {
                return Err(invalid("invalid tax collection identity, date or cash"));
            }
            last_collection_day = collection.day;
            let due = self
                .assessed_cents_through(collection.day, collection.operation_seq)?
                .checked_sub(collected)
                .ok_or_else(|| overflow("collection due subtraction"))?;
            let expected = due.min(i128::from(collection.available_cash.cents()));
            if i128::from(collection.collected.cents()) != expected
                || collection.remaining_cash.cents()
                    != collection.available_cash.cents() - collection.collected.cents()
                || collection.outstanding != ExactDividendTaxAmount::new(due - expected, 1)?
                || collection.needs_funds != (due > expected)
            {
                return Err(invalid(
                    "collection breaks actual cash or outstanding tax conservation",
                ));
            }
            collected = collected
                .checked_add(expected)
                .ok_or_else(|| overflow("collected tax total"))?;
        }
        Ok(())
    }

    fn advance_operation(&mut self) -> Result<u64, DividendTaxError> {
        self.operation_seq = self
            .operation_seq
            .checked_add(1)
            .ok_or_else(|| overflow("operation sequence"))?;
        Ok(self.operation_seq)
    }
}

pub(crate) fn convert_tax_source(
    source: &crate::company::share_registry::AcquisitionSource,
) -> crate::company::cash_dividend_tax::TaxAcquisitionSource {
    match source {
        crate::company::share_registry::AcquisitionSource::InitialAllocation { evidence } => {
            crate::company::cash_dividend_tax::TaxAcquisitionSource::InitialAllocation {
                evidence: evidence.clone(),
            }
        }
        crate::company::share_registry::AcquisitionSource::SecondaryMarket { settlement } => {
            crate::company::cash_dividend_tax::TaxAcquisitionSource::SecondaryMarket {
                settlement: settlement.clone(),
            }
        }
        crate::company::share_registry::AcquisitionSource::CorporateAction { event } => {
            crate::company::cash_dividend_tax::TaxAcquisitionSource::CorporateAction {
                event: event.clone(),
            }
        }
    }
}

pub(crate) fn convert_tax_class(
    restriction: &crate::company::share_registry::ShareRestriction,
) -> crate::company::cash_dividend_tax::TaxShareClass {
    match restriction {
        crate::company::share_registry::ShareRestriction::Unrestricted => {
            crate::company::cash_dividend_tax::TaxShareClass::PublicMarket
        }
        crate::company::share_registry::ShareRestriction::Restricted { reason, release_on } => {
            crate::company::cash_dividend_tax::TaxShareClass::StatutoryRestricted {
                release_on: *release_on,
                basis:
                    crate::company::cash_dividend_tax::StatutoryRestrictedBasis::FinanceTax2009167,
                qualification_evidence: reason.to_string(),
            }
        }
    }
}

/// 个人流通股现金分红差别化税率：持股期限以转让交割日前一日截止，
/// “一个月”指上月某日至本月同日前一日（财税〔2012〕85号第八条）。
/// 目标月无对应日（如1月31日→2月、2月29日取得跨平年）时，按《民法典》
/// 期间计算规则把边界钳制到目标月最后一日，而不是对整类批次报错。
pub fn personal_cash_dividend_rate(
    acquired_on: CivilDate,
    disposed_on: CivilDate,
) -> Result<u8, DividendTaxError> {
    if disposed_on < acquired_on {
        return Err(invalid("disposal must follow acquisition"));
    }
    if disposed_on <= clamped_anniversary(acquired_on, 1)? {
        return Ok(20);
    }
    let year_boundary = clamped_anniversary(acquired_on, 12)?;
    Ok(if disposed_on <= year_boundary { 10 } else { 0 })
}

/// 取得日加 `add_months` 个自然月后的对应日；无对应日时取目标月最后一日。
fn clamped_anniversary(
    acquired_on: CivilDate,
    add_months: u32,
) -> Result<CivilDate, DividendTaxError> {
    let total_months = i64::from(acquired_on.month()) - 1 + i64::from(add_months);
    let year = i64::from(acquired_on.year())
        .checked_add(total_months.div_euclid(12))
        .ok_or_else(|| overflow("holding period anniversary year"))?;
    let month = u8::try_from(total_months.rem_euclid(12) + 1)
        .map_err(|_| invalid("holding period anniversary month"))?;
    let year = i32::try_from(year).map_err(|_| invalid("holding period anniversary year range"))?;
    let last_day = crate::calendar::days_in_month(year, month);
    CivilDate::from_ymd(year, month, acquired_on.day().min(last_day))
        .map_err(|error| invalid(&format!("holding period anniversary date: {error}")))
}

fn total_quantity(lots: &[DividendTaxLot]) -> Result<u64, DividendTaxError> {
    lots.iter().try_fold(0u64, |total, lot| {
        total
            .checked_add(lot.qty)
            .ok_or_else(|| overflow("tax lot share sum"))
    })
}

/// 缩股核减的 FIFO 消耗：从最旧 lot 开始消耗（限售类允许被核减——重新计值
/// 不是处置），返回核减片段；数量不足显式失败。
fn apply_redenomination_reduction(
    lots: &mut Vec<DividendTaxLot>,
    day: CivilDate,
    removed_qty: u64,
) -> Result<Vec<TaxDisposition>, DividendTaxError> {
    let mut remaining = removed_qty;
    let mut removed = Vec::new();
    for lot in lots.iter_mut() {
        if remaining == 0 {
            break;
        }
        let consumed = remaining.min(lot.qty);
        let mut reduced = lot.clone();
        reduced.qty = consumed;
        removed.push(TaxDisposition {
            lot: reduced,
            disposed_on: day,
        });
        lot.qty -= consumed;
        remaining -= consumed;
    }
    if remaining != 0 {
        return Err(invalid(
            "insufficient tax lots for share re-denomination reduction",
        ));
    }
    lots.retain(|lot| lot.qty != 0);
    validate_lots(lots, day)?;
    Ok(removed)
}

fn apply_net_change(
    lots: &mut Vec<DividendTaxLot>,
    day: CivilDate,
    net_change: i128,
    acquisition: Option<&DividendTaxLot>,
) -> Result<Vec<TaxDisposition>, DividendTaxError> {
    if (net_change > 0) != acquisition.is_some() {
        return Err(invalid(
            "net acquisition facts do not match positive share change",
        ));
    }
    let mut dispositions = Vec::new();
    if let Some(lot) = acquisition {
        validate_lot(lot, day)?;
        if lot.acquired_on != day || i128::from(lot.qty) != net_change {
            return Err(invalid("net acquisition date or quantity mismatch"));
        }
        lots.push(lot.clone());
    } else if net_change < 0 {
        let public = lots
            .iter()
            .any(|lot| lot.class == TaxShareClass::PublicMarket);
        let restricted = lots
            .iter()
            .any(|lot| matches!(lot.class, TaxShareClass::StatutoryRestricted { .. }));
        if public && restricted {
            return Err(DividendTaxError::UnsupportedMixedRestrictedTaxLots);
        }
        let mut remaining = u64::try_from(
            net_change
                .checked_neg()
                .ok_or_else(|| overflow("negative net shares"))?,
        )
        .map_err(|_| invalid("daily net disposal exceeds u64"))?;
        for lot in lots.iter_mut() {
            if remaining == 0 {
                break;
            }
            if matches!(&lot.class, TaxShareClass::StatutoryRestricted { release_on, .. } if day < *release_on)
            {
                return Err(invalid(
                    "statutory restricted shares cannot be publicly disposed before release",
                ));
            }
            let consumed = remaining.min(lot.qty);
            let mut disposed_lot = lot.clone();
            disposed_lot.qty = consumed;
            dispositions.push(TaxDisposition {
                lot: disposed_lot,
                disposed_on: day,
            });
            lot.qty -= consumed;
            remaining -= consumed;
        }
        if remaining != 0 {
            return Err(invalid("insufficient shares for daily tax disposal"));
        }
        lots.retain(|lot| lot.qty != 0);
    }
    validate_lots(lots, day)?;
    Ok(dispositions)
}
fn validate_lots(lots: &[DividendTaxLot], day: CivilDate) -> Result<(), DividendTaxError> {
    let mut ids = BTreeSet::new();
    let mut previous_date = None;
    for lot in lots {
        validate_lot(lot, day)?;
        if !ids.insert(&lot.id) || previous_date.is_some_and(|previous| previous > lot.acquired_on)
        {
            return Err(invalid(
                "tax lots have duplicate identities or unordered acquisition dates",
            ));
        }
        previous_date = Some(lot.acquired_on);
    }
    total_quantity(lots)?;
    Ok(())
}
fn validate_lot(lot: &DividendTaxLot, day: CivilDate) -> Result<(), DividendTaxError> {
    if lot.id.trim().is_empty() || lot.qty == 0 || lot.acquired_on > day {
        return Err(invalid(
            "tax lot requires identity, positive shares and valid explicit acquisition date",
        ));
    }
    let evidence = match &lot.source {
        TaxAcquisitionSource::InitialAllocation { evidence } => evidence,
        TaxAcquisitionSource::SecondaryMarket { settlement } => settlement,
        TaxAcquisitionSource::CorporateAction { event } => event,
    };
    if evidence.trim().is_empty() {
        return Err(invalid("tax lot acquisition source evidence is required"));
    }
    if let TaxShareClass::StatutoryRestricted {
        release_on,
        qualification_evidence,
        ..
    } = &lot.class
    {
        if *release_on < lot.acquired_on || qualification_evidence.trim().is_empty() {
            return Err(invalid(
                "statutory restricted tax lot requires qualifying evidence and release date",
            ));
        }
    }
    Ok(())
}
fn required_lot<'de, Decoder: serde::Deserializer<'de>>(
    decoder: Decoder,
) -> Result<Option<DividendTaxLot>, Decoder::Error> {
    Option::<DividendTaxLot>::deserialize(decoder)
}
fn invalid(detail: &str) -> DividendTaxError {
    DividendTaxError::InvalidFact(detail.into())
}
fn overflow(operation: &'static str) -> DividendTaxError {
    DividendTaxError::Overflow { operation }
}
fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

mod nonnegative_decimal {
    pub fn serialize<Serializer: serde::Serializer>(
        number: &i128,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error> {
        serializer.serialize_str(&number.to_string())
    }
    pub fn deserialize<'de, Decoder: serde::Deserializer<'de>>(
        decoder: Decoder,
    ) -> Result<i128, Decoder::Error> {
        let text = <String as serde::Deserialize>::deserialize(decoder)?;
        if text.is_empty()
            || !text.bytes().all(|byte| byte.is_ascii_digit())
            || (text.len() > 1 && text.starts_with('0'))
        {
            return Err(serde::de::Error::custom(
                "exact amount numerator must be a canonical nonnegative decimal string",
            ));
        }
        text.parse::<i128>().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
