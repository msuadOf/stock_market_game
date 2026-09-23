//! 策略身份档案：风格枚举、策略族与可存档的 StrategyProfile。

/// 独立自然人散户的长期行为风格。风格只决定参数分布，不共享账户、库存或 RNG。
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum RetailStyle {
    Dormant,
    LongTerm,
    Noise,
    DipBuyer,
    Momentum,
    Panic,
}

/// 独立机构账户的投资风格。五个默认机构按账户序号轮换，账户与库存不共享。
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum InstitutionStyle {
    DeepValue,
    Growth,
    Balanced,
    Defensive,
    ActiveTrader,
}

/// 独立游资账户的短线风格。
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum HotStyle {
    Momentum,
    Reversal,
}

/// 账户身份之外的决策策略族。
///
/// 身份仍决定账户规模和会话编排；策略能力单独决定可见数据，策略族说明如何从其允许的观测生成委托。
/// 二者分开后，一个积极交易型机构可以使用动量，而不会被重解释成游资账户。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StrategyFamily {
    RetailBehavior,
    FundamentalValue,
    Momentum,
}

/// 账户策略的可恢复身份档案；运行期 trait 对象不得作为唯一的存档依据。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum StrategyProfile {
    Retail(RetailStyle),
    Institution(InstitutionStyle),
    Hot(HotStyle),
}
